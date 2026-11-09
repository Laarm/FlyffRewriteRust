//! The guild bank handlers (Plan/21 §5.9; ref/07a §15.1-§15.5; `CDPSrvr::OnOpenGuildBankWnd .. OnGuildBankMoveItem`,
//! `DPSrvr.cpp:3265-3830`): the window, the put, the take (penya and item) and the move.
//!
//! Every handler passes the common gate first (`events.guild_bank` on, the actor belongs to a guild, see
//! [`member`]). A refused request changes nothing; the refusals that the original answered with a text still do
//! (`TID_GAME_CANNOT_DO_USINGITEM`, `TID_GAME_EQUIPTRADE`, `TID_GAME_GUILDBANKFULL`, `BankIsFull`), every other one is
//! silent. Put and take re-check the banker NPC on every request (VR-012), open and move do not (KEEP); put, take and
//! move need the window to be open (FIX VR-016: the original checked nothing).

use super::{
    BANK_SLOTS, Member, clamp_num, emit_bank, emit_move, member, moved_part, near_banker, remember,
    tell_viewers, window_open,
};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::container::{StorageId, SwapOutcome};
use crate::item::gold::{add_gold, can_add};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, RemoveReason, remove_item};
use crate::social::guild::commit::{self, Origin};
use crate::social::guild::model::GuildPower;
use crate::social::guild::wire;
use flyff_common::ObjId;
use flyff_common::defs::itemkind::IK3_QUEST;
use flyff_common::defs::text::{
    TID_GAME_CANNOT_DO_USINGITEM, TID_GAME_EQUIPTRADE, TID_GAME_GUILDBANKFULL,
};
use flyff_db::{AtomicBatch, DbOp, GuildBankLogKind, GuildBankLogRecord};
use flyff_proto::pb;
use flyff_proto::pb::get_item_guild_bank_ntf as get;

// ---- the window --------------------------------------------------------------------------------------------------

/// `GuildBankWndReq` (`OnOpenGuildBankWnd`, ref/07a §15.1): refused while trading, vending or with the personal bank
/// open; a player without a guild is ignored and the flag stays false (FIX: the original set it anyway). The window
/// opens with `GuildBankWndNtf{mode 0, gold, items}`; no banker NPC is needed to OPEN it (KEEP).
pub fn on_guild_bank_wnd_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::GuildBankWndReq,
) {
    let Some(me) = member(svc, ctx, who) else {
        return;
    };
    let Some(p) = ctx.player(who).and_then(|mv| mv.player()) else {
        return;
    };
    if p.econ.has_partner() || p.econ.vending || p.econ.bank.is_some() || p.econ.bank_open {
        return;
    }
    let Some(g) = svc.social.guild.get(me.guild) else {
        return;
    };
    let msg = pb::GuildBankWndNtf {
        mode: 0,
        gold: g.gold,
        items: Some(g.bank.to_pb()),
    };
    if let Some(p) = ctx.player_mut(who).and_then(|mv| mv.player_mut()) {
        p.social.guild_bank_open = true;
    }
    wire::to_one(ctx, who.0, msg);
}

/// `GuildBankWndClose` (`OnCloseGuildBankWnd`, ref/07a §15.2): the window closes. Not gated: the flag is only the
/// mirror of the client's window, and a window that cannot be closed helps nobody.
pub fn on_guild_bank_wnd_close(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::GuildBankWndClose,
) {
    if let Some(p) = ctx.player_mut(who).and_then(|mv| mv.player_mut()) {
        p.social.guild_bank_open = false;
    }
}

// ---- put ---------------------------------------------------------------------------------------------------------

/// `PutItemGuildBankReq{obj_id, count, mode}` (`OnPutItemGuildBank`, ref/07a §15.3). Any member may deposit (no rank
/// check). The checks in this order, any failure changes nothing: `mode == 1` (gold deposits are refused), the window
/// is open (FIX VR-016), a banker within range, then the item (usable, not a quest item, not bound, not in use, not
/// a vagrant ride, not worn, not charged), the bank has room, the session can be saved.
///
/// Commit: the bank takes `n` pieces (a part of a stack gets a fresh serial, a whole stack keeps its own), the
/// inventory loses them through the normal `UpdateItem` / `DeleteItem`, ONE `GuildBankPutItem` batch carries the
/// member's snapshot, the whole bank document and the log row `A`. Messages: `PutItemGuildBankNtf{mode 1}` to the
/// actor, `{mode 3}` to the window-open members inside the actor's observer set (encoded once).
pub fn on_put_item_guild_bank(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::PutItemGuildBankReq,
) {
    if m.mode != 1 {
        return;
    }
    let Some(me) = member(svc, ctx, who) else {
        return;
    };
    if !window_open(ctx, who) || !near_banker(ctx, who) {
        return;
    }
    let Ok(inv_id) = StorageId::try_from(m.obj_id) else {
        return;
    };
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    // ---- validation on the immutable state ----
    let (incoming, n, pack_max) = {
        let Some(p) = ctx.player(who).and_then(|mv| mv.player()) else {
            return;
        };
        let Some(item) = p.items.inventory.get_at_id(inv_id) else {
            return;
        };
        if !item.is_usable() {
            return;
        }
        let Some(prop) = env.data().item(item.item_id) else {
            return;
        };
        if prop.item_kind3 == IK3_QUEST || item.is_binds(prop, env.cfg().only_armor_awakes) {
            return;
        }
        if p.items.is_using(inv_id) {
            notice::defined(ctx, who, TID_GAME_CANNOT_DO_USINGITEM, Vec::new());
            return;
        }
        if Item::is_vagrant_ride(prop) {
            return;
        }
        if p.items.inventory.is_equip(inv_id) {
            notice::defined(ctx, who, TID_GAME_EQUIPTRADE, Vec::new());
            return;
        }
        if item.is_charged() {
            return;
        }
        let n = clamp_num(m.count, item.count);
        (moved_part(item, n), n, prop.pack_max.max(1))
    };
    let full = svc
        .social
        .guild
        .get(me.guild)
        .is_none_or(|g| g.bank.items.is_full(&incoming, pack_max, u32::from(n)));
    if full {
        notice::defined(ctx, who, TID_GAME_GUILDBANKFULL, Vec::new());
        return;
    }
    if !can_snapshot(ctx, who) {
        return;
    }
    // ---- the clone of the bank takes the item: the only step that can fail (a serial fault), nothing changed yet ----
    let Some(mut bank) = svc.social.guild.get(me.guild).map(|g| g.bank.clone()) else {
        return;
    };
    let plan = {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        match bank.items.add(&incoming, env.data(), &mut src) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::warn!(error = %e, "a guild bank put was refused by the container");
                return;
            }
        }
    };
    // ---- the commit block ----
    let mut sent = incoming;
    if sent.serial.0 == 0 {
        sent.serial = plan.template_serial;
    }
    let first = plan.touched.first().map_or(0, |t| t.0);
    let item_pb = sent.to_pb(first);
    if let Some(g) = svc.social.guild.get_mut(me.guild) {
        g.bank = bank;
    }
    remove_item(svc, ctx, who, inv_id, n, RemoveReason::Moved);
    let log = GuildBankLogRecord::new(me.guild, me.c, GuildBankLogKind::ItemPut, commit::now(ctx))
        .with_item(sent.item_id, sent.upgrade_level, i64::from(n));
    remember(svc, me.guild, &log);
    queue(svc, ctx, who, me, "GuildBankPutItem", |snap, doc| {
        AtomicBatch::guild_bank_put_item(snap, me.guild, doc, log)
    });
    wire::to_one(
        ctx,
        who.0,
        pb::PutItemGuildBankNtf {
            mode: 1,
            item: Some(item_pb.clone()),
        },
    );
    tell_viewers(
        svc,
        ctx,
        who,
        me.guild,
        pb::PutItemGuildBankNtf {
            mode: 3,
            item: Some(item_pb),
        },
    );
    emit_bank(
        ctx,
        Origin::Player(who),
        me.guild,
        me.c,
        GuildBankLogKind::ItemPut,
        Some(&sent),
        0,
    );
}

/// Takes the snapshot of `who` (after its inventory and purse changed) and the whole bank document, builds the
/// batch with `build` and queues it with its ticket. A missing snapshot cannot happen after `can_snapshot` and is
/// logged at error (the memory is kept).
fn queue(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    me: Member,
    label: &'static str,
    build: impl FnOnce(flyff_db::CharacterSnapshot, flyff_db::GuildBankDoc) -> AtomicBatch,
) {
    let doc = svc.social.guild.get(me.guild).map(|g| g.bank.to_doc());
    match (snapshot_for_batch(svc, ctx, who), doc) {
        (Some((snap, generation)), Some(doc)) => {
            commit::submit(
                svc,
                ctx,
                label,
                build(snap, doc),
                &[(who, generation)],
                None,
            );
        }
        _ => tracing::error!(
            guild = me.guild.0,
            batch = label,
            "a guild bank change could not be saved: no snapshot"
        ),
    }
}

// ---- take --------------------------------------------------------------------------------------------------------

/// `GetItemGuildBankReq{obj_id, count_or_gold, mode}` (`OnGetItemGuildBank`, ref/07a §15.4): mode 0 takes penya
/// (`PF_PENYA`), mode 1 an item (`PF_ITEM`); other modes are ignored. Both need the window, a banker in range and
/// the power; they are one batch each.
pub fn on_get_item_guild_bank(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GetItemGuildBankReq,
) {
    if m.mode > 1 {
        return;
    }
    let Some(me) = member(svc, ctx, who) else {
        return;
    };
    if !window_open(ctx, who) || !near_banker(ctx, who) {
        return;
    }
    if m.mode == 0 {
        take_penya(svc, ctx, who, me, m.count_or_gold);
    } else {
        take_item(svc, ctx, who, me, m.obj_id, m.count_or_gold);
    }
}

/// Mode 0: `n = (int) count_or_gold > 0`, `n <= guild.gold`, the purse can take it. Commit: `guild.gold -= n`, the
/// purse `+ n` WITHOUT a `SetPointParam` (the client adds the penya when it reads `penya_mine`), the member's
/// contribution `- n` (an `i32` that may go negative, KEEP), ONE `GuildBankTakeGold` batch with the log row `O`.
/// The actor gets `penya_mine`, every OTHER online member (window open or not, any world) `penya_all`.
fn take_penya(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    me: Member,
    count_or_gold: u32,
) {
    if !svc.social.guild.has_power(me.c, GuildPower::PENYA) {
        return;
    }
    let n = count_or_gold as i32;
    let Ok(amount) = u32::try_from(n) else {
        return;
    };
    if amount == 0 {
        return;
    }
    let Some(g) = svc.social.guild.get(me.guild) else {
        return;
    };
    let purse = ctx
        .player(who)
        .and_then(|mv| mv.player())
        .map_or(0, |p| i64::from(p.items.gold));
    if amount > g.gold || !can_add(purse, i64::from(n)) || !can_snapshot(ctx, who) {
        return;
    }
    // ---- the commit block ----
    if let Some(g) = svc.social.guild.get_mut(me.guild) {
        g.gold -= amount;
        if let Some(mm) = g.members.get_mut(&me.c) {
            mm.give_gold = mm.give_gold.saturating_sub(n);
        }
    }
    add_gold(svc, ctx, who, i64::from(n), false);
    let log = GuildBankLogRecord::new(me.guild, me.c, GuildBankLogKind::PenyaOut, commit::now(ctx))
        .with_penya(i64::from(n));
    remember(svc, me.guild, &log);
    let rows = svc.social.guild.get(me.guild).and_then(|g| {
        g.members
            .get(&me.c)
            .map(|mm| (g.to_record(), mm.to_record(me.guild)))
    });
    match (snapshot_for_batch(svc, ctx, who), rows) {
        (Some((snap, generation)), Some((guild_row, member_row))) => {
            commit::submit(
                svc,
                ctx,
                "GuildBankTakeGold",
                AtomicBatch::guild_bank_take_gold(snap, guild_row, member_row, log),
                &[(who, generation)],
                None,
            );
        }
        _ => tracing::error!(
            guild = me.guild.0,
            "a guild bank withdrawal could not be saved: no snapshot"
        ),
    }
    wire::to_one(
        ctx,
        who.0,
        pb::GetItemGuildBankNtf {
            sub: Some(get::Sub::PenyaMine(get::PenyaMineUpdate {
                gold: amount,
                id_player: me.c.0,
                cloak: false,
            })),
        },
    );
    let others: Vec<ObjId> = svc
        .social
        .guild
        .online_objs(&svc.presence, me.guild)
        .into_iter()
        .filter(|o| *o != who.0)
        .collect();
    wire::to_list(
        svc,
        ctx,
        &others,
        pb::GetItemGuildBankNtf {
            sub: Some(get::Sub::PenyaAll(get::PenyaAllUpdate {
                gold: amount,
                id_player: me.c.0,
                cloak: false,
            })),
        },
    );
    emit_bank(
        ctx,
        Origin::Player(who),
        me.guild,
        me.c,
        GuildBankLogKind::PenyaOut,
        None,
        i64::from(n),
    );
}

/// Mode 1: `obj_id < 42` and occupied, `n = count.clamp(1, stack)`, the inventory must take the item (else
/// `BankIsFull` and nothing changes). A PARTIAL take splits: the taken part gets a fresh serial (the original copied
/// the serial, `db check --serials` must stay clean). The inventory add is SILENT: the client adds the item itself
/// when it reads `item_mine` (whose `obj_id` is the BANK object id and whose count is `n`). ONE `GuildBankTakeItem`
/// batch with the log row `D`; `item_all` goes to the window-open members inside the actor's observer set.
fn take_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    me: Member,
    obj_id: u32,
    count: u32,
) {
    if !svc.social.guild.has_power(me.c, GuildPower::ITEM) {
        return;
    }
    let Ok(bid) = StorageId::try_from(obj_id) else {
        return;
    };
    if bid >= BANK_SLOTS {
        return;
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let (incoming, n, held) = {
        let Some(item) = svc
            .social
            .guild
            .get(me.guild)
            .and_then(|g| g.bank.items.get_at_id(bid))
        else {
            return;
        };
        let n = clamp_num(count, item.count);
        (moved_part(item, n), n, item.count)
    };
    let pack_max = env
        .data()
        .item(incoming.item_id)
        .map_or(1, |p| p.pack_max.max(1));
    let fits = ctx
        .player(who)
        .and_then(|mv| mv.player())
        .is_some_and(|p| !p.items.inventory.is_full(&incoming, pack_max, u32::from(n)));
    if !fits {
        wire::to_one(ctx, who.0, pb::BankIsFull {});
        return;
    }
    if !can_snapshot(ctx, who) {
        return;
    }
    let Some(mut bank) = svc.social.guild.get(me.guild).map(|g| g.bank.clone()) else {
        return;
    };
    if n >= held {
        bank.items.remove_at_id(bid);
    } else if let Some(it) = bank.items.get_at_id_mut(bid) {
        it.count = held - n;
    }
    // the bag first: the only step that can fail (a serial fault), nothing has changed then
    let plan = {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        let Some(p) = ctx.worlds.player_mut(who.0).and_then(|mv| mv.player_mut()) else {
            return;
        };
        match p.items.inventory.add(&incoming, env.data(), &mut src) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::warn!(error = %e, "a guild bank take was refused by the inventory");
                return;
            }
        }
    };
    // ---- the commit block ----
    let mut sent = incoming;
    if sent.serial.0 == 0 {
        sent.serial = plan.template_serial;
    }
    let item_pb = sent.to_pb(bid);
    if let Some(g) = svc.social.guild.get_mut(me.guild) {
        g.bank = bank;
    }
    let log = GuildBankLogRecord::new(
        me.guild,
        me.c,
        GuildBankLogKind::ItemTaken,
        commit::now(ctx),
    )
    .with_item(sent.item_id, sent.upgrade_level, i64::from(n));
    remember(svc, me.guild, &log);
    queue(svc, ctx, who, me, "GuildBankTakeItem", |snap, doc| {
        AtomicBatch::guild_bank_take_item(snap, me.guild, doc, log)
    });
    wire::to_one(
        ctx,
        who.0,
        pb::GetItemGuildBankNtf {
            sub: Some(get::Sub::ItemMine(get::ItemMineUpdate {
                item: Some(item_pb.clone()),
            })),
        },
    );
    tell_viewers(
        svc,
        ctx,
        who,
        me.guild,
        pb::GetItemGuildBankNtf {
            sub: Some(get::Sub::ItemAll(get::ItemAllUpdate {
                item: Some(item_pb),
            })),
        },
    );
    emit_bank(
        ctx,
        Origin::Player(who),
        me.guild,
        me.c,
        GuildBankLogKind::ItemTaken,
        Some(&sent),
        0,
    );
}

// ---- move --------------------------------------------------------------------------------------------------------

/// `GuildBankMoveItem{src, dst}` (`OnGuildBankMoveItem`, ref/07a §15.5): the window is open, both slots are below
/// 42 and differ (FIX VR-016: the original checked nothing). `ItemContainer::swap` merges compatible stacks like the
/// client's own swap. The whole document goes out as ONE `SetGuildBank` op (no snapshot, no log row, audit kind
/// `move`); NO client message (KEEP-QUIRK KQ-39 e: the client moved its own mirror already). No banker is needed.
pub fn on_guild_bank_move_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildBankMoveItem,
) {
    let Some(me) = member(svc, ctx, who) else {
        return;
    };
    if !window_open(ctx, who) {
        return;
    }
    let slots = u32::from(BANK_SLOTS);
    if m.src >= slots || m.dst >= slots || m.src == m.dst {
        return;
    }
    let (Ok(src), Ok(dst)) = (u16::try_from(m.src), u16::try_from(m.dst)) else {
        return;
    };
    let Some(mut bank) = svc.social.guild.get(me.guild).map(|g| g.bank.clone()) else {
        return;
    };
    if bank.items.swap(src, dst, ctx.data) == SwapOutcome::Nothing {
        return;
    }
    let doc = bank.to_doc();
    if let Some(g) = svc.social.guild.get_mut(me.guild) {
        g.bank = bank;
    }
    ctx.persist.op(DbOp::SetGuildBank {
        guild_id: me.guild,
        doc,
    });
    emit_move(ctx, Origin::Player(who), me.guild, me.c);
}
