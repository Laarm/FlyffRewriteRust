//! The chips, the close wait, the close, the all-close and the sweep of an arena (WP-SOCIAL-26;
//! `GuildCombat1to1.cpp:255-387`, `1153-1174`).

use super::{
    MADRIGAL_SPOT, arena_clone, arena_mut, arena_state_msg, present_members, send_state_all,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemError, ItemOrigin};
use crate::social::gc1to1::{
    ArenaState, FIRST_ARENA, Gc1to1State, defined, send_manager_state, send_one, small_caption,
    submit_then, tender, text_of,
};
use crate::social::siege::fight::{live_avatar, set_gc_state};
use crate::world::ReplaceKind;
use flyff_common::defs::item::II_CHP_RED;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT1TO1_WARGUILDDRAW, TID_GAME_GUILDCOMBAT1TO1_WARGUILDLOST,
    TID_GAME_GUILDCOMBAT1TO1_WARGUILDWIN, TID_GAME_GUILDCOMBAT1TO1_WARGUILDWINCHIP,
    TID_GAME_GUILDCOMBAT1TO1_WINCHIPPOST,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::{CharacterId, GameMs, LayerId, MailId, ObjId, WorldId};
use flyff_db::{AtomicBatch, AtomicKind, DbOp, SaveReason, TenderState};
use flyff_rules::social::gc1to1::win_chips;
use serde_json::json;

// ---- the chips ------------------------------------------------------------------------------------------------------------------

/// What the chip deliveries of one commit block left for its batch.
#[derive(Default)]
pub(super) struct Awards {
    /// Recipients whose inventory changed (they need a snapshot in the batch).
    changed: Vec<PlayerRef>,
    /// The mails of the awards that could not be delivered in the inventory.
    mails: Vec<MailId>,
    /// The ops of those mails.
    ops: Vec<DbOp>,
    /// `(character, chips, mailed)` of every award.
    given: Vec<(CharacterId, i32, bool)>,
}

/// The chip item (`II_CHP_RED`, `m_bCharged` from the prop).
fn chip_item(ctx: &GameCtx<'_>, n: u16) -> Item {
    let mut item = Item::new(II_CHP_RED, n);
    item.charged = ctx.data.item(II_CHP_RED).is_some_and(|p| p.charged != 0);
    item
}

/// `SetWinChip`'s delivery (`269-300`): `n` red chips into the inventory of the online `c`; on a full inventory (or a
/// session that cannot be snapshotted) a system mail whose title is the item's name and whose text is
/// `WINCHIPPOST`. Memory only: [`flush_awards`] commits the batch.
pub(super) fn give_chips(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    obj: ObjId,
    chips: i32,
    out: &mut Awards,
) {
    let n = u16::try_from(chips.max(1)).unwrap_or(u16::MAX);
    let who = PlayerRef(obj);
    let item = chip_item(ctx, n);
    if can_snapshot(ctx, who) {
        match crate::item::ops::create_item(
            svc,
            ctx,
            who,
            item.clone(),
            ItemOrigin::Other("gc1to1_chip"),
        ) {
            Ok(_) => {
                out.changed.push(who);
                out.given.push((c, chips, false));
                return;
            }
            Err(ItemError::LackSpace) => {}
            Err(e) => {
                tracing::error!(error = %e, character = c.0, "GC 1:1 chips: the item could not be created");
                return;
            }
        }
    }
    let mut item = item;
    if item.serial.0 == 0 {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        match crate::item::container::SerialSource::next_serial(&mut src) {
            Ok(s) => item.serial = flyff_common::ItemSerial(s),
            Err(e) => {
                tracing::error!(error = %e, character = c.0, "GC 1:1 chips: no serial for the mail");
                return;
            }
        }
    }
    let name = ctx
        .data
        .item(II_CHP_RED)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let body = text_of(ctx, TID_GAME_GUILDCOMBAT1TO1_WINCHIPPOST);
    let (id, ops) = svc
        .econ
        .mail
        .send_system_with_id(c, &name, &body, Some(item), 0);
    out.mails.push(id);
    out.ops.extend(ops);
    out.given.push((c, chips, true));
}

/// Commits the deliveries of a block with `extra` ops in ONE `AtomicBatch::of_ops(Other(label), ..)`: the snapshots of
/// every recipient whose inventory changed, `extra`, then the mail ops (as the siege-b `results` pattern); the save
/// scheduler learns of every snapshot. On a failed batch the chip mails leave the memory (CRITICAL log, the rest of the
/// memory is kept). Nothing happens when there is nothing to write.
pub(super) fn flush_awards(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    label: &'static str,
    awards: Awards,
    extra: Vec<DbOp>,
) {
    let mut ops = extra;
    ops.extend(awards.ops);
    let mut batch = AtomicBatch::of_ops(AtomicKind::Other(label), ops);
    let mut generations = Vec::new();
    for who in &awards.changed {
        match snapshot_for_batch(svc, ctx, *who) {
            Some((snap, generation)) => {
                batch.snapshots.push(snap);
                generations.push((*who, generation));
            }
            None => tracing::error!(
                player = who.0.0,
                "GC 1:1 chips: a recipient could not be snapshotted: the next save keeps his chips"
            ),
        }
    }
    if batch.snapshots.is_empty() && batch.ops.is_empty() {
        return;
    }
    let mails = awards.mails;
    submit_then(ctx, label, batch, move |svc| {
        for id in &mails {
            svc.econ.mail.remove(*id);
        }
    });
    for (who, generation) in generations {
        svc.saves
            .record(who.0, generation, ctx.clock.game_ms, SaveReason::Atomic);
    }
}

// ---- the close -------------------------------------------------------------------------------------------------------------------

/// `GuildCombat1to1CloseWait` (`317-366`): state CloseWait for `WarCloseWaitTime` (`immediate`: due now, the GM close), the
/// state to all; the guild with more round wins wins (equal = draw). Each present line-up member: the winner side gets the
/// chips (no box), the small caption `WARGUILDWIN` and `WARGUILDWINCHIP [chips]`; a draw the caption `WARGUILDDRAW`; the
/// loser `WARGUILDLOST`; then his state. ONE `AtomicBatch::of_ops(Other("Gc1to1Result"), ..)` with both tender rows
/// `T -> E`, the chip snapshots and the chip mails; the audit line `gc1to1.result`.
pub(crate) fn close_wait(svc: &mut Services, ctx: &mut GameCtx<'_>, idx: usize, immediate: bool) {
    let now = ctx.clock.game_ms;
    let wait = svc.social.gc1to1.config.war_close_wait_time_ms;
    if let Some(m) = arena_mut(svc, idx) {
        m.state = ArenaState::CloseWait;
        m.due = if immediate {
            now
        } else {
            GameMs(now.0.saturating_add(wait))
        };
        m.ended = true;
    }
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    send_state_all(svc, ctx, &a);
    let winner = match a.side(0).wins.cmp(&a.side(1).wins) {
        std::cmp::Ordering::Greater => Some(0),
        std::cmp::Ordering::Less => Some(1),
        std::cmp::Ordering::Equal => None,
    };
    let chips = win_chips(svc.social.gc1to1.config.win_chip_num);
    let mut awards = Awards::default();
    for (side, c, obj) in present_members(svc, ctx, &a) {
        if Some(side) == winner {
            give_chips(svc, ctx, c, obj, chips, &mut awards);
            small_caption(ctx, obj, TID_GAME_GUILDCOMBAT1TO1_WARGUILDWIN, Vec::new());
            defined(
                ctx,
                obj,
                TID_GAME_GUILDCOMBAT1TO1_WARGUILDWINCHIP,
                vec![chips.to_string()],
            );
        } else if winner.is_none() {
            small_caption(ctx, obj, TID_GAME_GUILDCOMBAT1TO1_WARGUILDDRAW, Vec::new());
        } else {
            small_caption(ctx, obj, TID_GAME_GUILDCOMBAT1TO1_WARGUILDLOST, Vec::new());
        }
        send_one(ctx, obj, arena_state_msg(&a, now));
    }
    // both tenders T -> E
    let mut ops = Vec::new();
    for side in 0..2 {
        let guild = a.side(side).guild;
        let Some(t) = svc.social.gc1to1.tender_of(guild).cloned() else {
            continue;
        };
        ops.push(DbOp::UpsertGc1to1Tender(tender::set_state(
            Some(TenderState::Tendered),
            TenderState::Ended,
            t.round,
            guild,
            t.penya,
            t.tendered_at,
        )));
        svc.social.gc1to1.spent.insert(guild);
    }
    let given = awards.given.clone();
    flush_awards(svc, ctx, "Gc1to1Result", awards, ops);
    ctx.audit.emit(flyff_db::AuditEvent::new(
        "gc1to1.result",
        json!({
            "world": a.world.0,
            "round": svc.social.gc1to1.round(),
            "guilds": [a.side(0).guild.0, a.side(1).guild.0],
            "wins": [a.side(0).wins, a.side(1).wins],
            "winner": winner.map(|s| a.side(s).guild.0),
            "chips": given
                .iter()
                .map(|(c, n, mailed)| json!({"character": c.0, "chips": n, "mailed": mailed}))
                .collect::<Vec<_>>(),
        }),
    ));
}

/// `GuildCombat1to1Close` (`369-387`): the arena is closed; each present line-up member hears `{100, 0}`, loses his combat
/// state and goes to Madrigal (6983, 0, 3326), layer 0. FIX: the replace is forced (a flying player stayed in the arena).
pub(super) fn close(svc: &mut Services, ctx: &mut GameCtx<'_>, idx: usize) {
    if let Some(m) = arena_mut(svc, idx) {
        m.state = ArenaState::Close;
        m.due = GameMs(0);
    }
    let Some(a) = arena_clone(svc, idx) else {
        return;
    };
    for (_, _, obj) in present_members(svc, ctx, &a) {
        send_one(ctx, obj, arena_state_msg(&a, ctx.clock.game_ms));
        set_gc_state(ctx, obj, 0);
        if let Err(e) = ctx.worlds.replace(
            obj,
            WorldId(WI_WORLD_MADRIGAL),
            MADRIGAL_SPOT,
            LayerId(0),
            ReplaceKind::Force,
        ) {
            tracing::debug!(error = %e, "a GC 1:1 closing teleport was refused");
        }
    }
}

/// `GuildCombat1to1AllClose` (`1153-1174`): state Close, `{0, 0}` to every online member of every tendering guild, the
/// arenas and the tenders cleared, the round counted on (`DbManager.cpp:7934-7940`).
pub(crate) fn all_close(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    {
        let g = &mut svc.social.gc1to1;
        g.set_state(Gc1to1State::Close);
        g.due = GameMs(0);
    }
    for (c, obj) in tender::tender_member_avatars(svc, ctx) {
        send_manager_state(svc, ctx, obj, c);
    }
    let g = &mut svc.social.gc1to1;
    g.arenas.clear();
    g.tenders.clear();
    g.spent.clear();
    g.round = g.round.saturating_add(1);
}

// ---- the sweep -------------------------------------------------------------------------------------------------------------------

/// FIX of `World.cpp:1628-1635`, which reset the combat state only on a replace to Madrigal: while the event runs, every
/// online line-up member with a combat state outside the arena worlds 241..=244 gets state 0 (the world seam
/// `WorldHooks::replace_leaving` has no `Services` and no sink for the `GcUserState` broadcast, so the manager's slot does
/// it).
pub fn sweep(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let members: Vec<CharacterId> = svc
        .social
        .gc1to1
        .tenders()
        .iter()
        .flat_map(|t| t.lineup.iter().copied())
        .collect();
    for c in members {
        let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) else {
            continue;
        };
        let stale = ctx.worlds.player(obj).is_some_and(|m| {
            m.player().is_some_and(|p| p.social.guild_combat_state != 0)
                && !(FIRST_ARENA..FIRST_ARENA + crate::social::gc1to1::ARENA_WORLDS)
                    .contains(&m.core.world.0)
        });
        if stale {
            set_gc_state(ctx, obj, 0);
        }
    }
}
