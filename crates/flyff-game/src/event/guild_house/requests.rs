//! The client messages of the guild house and the item rows (Plan/22 §12.2-§12.4; VR-012, VR-004, VR-066, PL-074,
//! PL-107; `_Common/GuildHouse.cpp:349-556, 752-970, 1197-1306, 1342-1444`).
//!
//! * `GuildHouseBuy` ([`on_buy`]): silently ignored without the sale NPC (menu 147) in range (A: the original had no NPC
//!   check); then each refusal is one `DefinedText` and nothing is charged, in the order of `IsBuyAble`.
//! * `GuildHouseEnter` ([`on_enter`]) / the comeback scroll ([`use_row`] -> [`on_item_followup`]): one [`enter`].
//! * `GuildHouseGoOut` ([`on_go_out`]).
//! * `GuildHousePacketReq` ([`on_packet_req`]): only SETUP, REMOVE, RESET and UPKEEP are accepted; every other op is
//!   ignored (FIX: the original forwarded a client `LISTDROP` or `EXPIRED` to the database). The entry acted on is the
//!   SERVER's list entry at `index`: the client's item id and keep time are ignored (VR-066, Q5).
//! * list-up ([`use_row`] queues [`crate::follow::Followup::GuildHouseItem`], [`on_item_followup`] runs it).

use super::members::{apply_entry_to_members, apply_house, reset_entry_for_members, source_of};
use super::room::{check_destroy_room, ensure_room, go_out, is_inside, spawn_object};
use super::{
    BUY_MIN_LEVEL, BUY_MIN_MEMBERSIZE, BUY_PENYA, BUY_UPKEEP_DAYS, COMEBACK_POS, ENTER_X, ENTER_Y,
    ENTER_Z, GhFurniture, GhOp, GuildHouse, MAX_LISTUP_NUM, MAX_UPKEEP_DAY, TELEPORTER_POS,
    UPKEEP_DAY_PENYA_SMALL, UPKEEP_DAY_TIME, WORLD, all_info, fntr_info, layer_of, log_result,
    rest::REST_POINT_TICK_MS, room_live,
};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::follow::Followup;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{RemoveReason, remove_item};
use crate::item::usage::{RowCx, RowResult};
use crate::mover::Mover;
use crate::world::ReplaceKind;
use flyff_common::defs::item::II_GHOU_FUR_NPC_TELEPORTER;
use flyff_common::defs::itemkind::{
    IK2_GUILDHOUES_COMEBACK, IK2_GUILDHOUSE_FURNITURE, IK2_GUILDHOUSE_NPC, IK2_GUILDHOUSE_PAPERING,
    IK3_CHAIR, IK3_TELEPORTER,
};
use flyff_common::defs::neuz::{MMI_GUILDHOUSE_ENTER, MMI_GUILDHOUSE_SALE};
use flyff_common::defs::text::{
    TID_GAME_GUILDHOUSE_BUY_EXPENSE, TID_GAME_GUILDHOUSE_BUY_HAVE, TID_GAME_GUILDHOUSE_BUY_LEVEL,
    TID_GAME_GUILDHOUSE_BUY_MASTER, TID_GAME_GUILDHOUSE_BUY_POPULATION,
    TID_GAME_GUILDHOUSE_COMEBACK_ERROR01, TID_GAME_GUILDHOUSE_ENTER_HAVE,
    TID_GAME_GUILDHOUSE_EXPIRATION, TID_GAME_GUILDHOUSE_FUR_INSTALL_LEVEL,
    TID_GAME_GUILDHOUSE_INSIDE_INSTALL_KIND, TID_GAME_GUILDHOUSE_INSIDE_INSTALL_LEVEL,
    TID_GAME_GUILDHOUSE_INSIDE_SPACE, TID_GAME_GUILDHOUSE_PURCHASE, TID_GAME_GUILDHOUSE_TAX_LEVEL,
    TID_GAME_GUILDHOUSE_TAX_MAX,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, GuildId, UnixTime, WorldId};
use flyff_data::props::item::ItemProp;
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, AuditEvent, DbOp};
use flyff_proto::pb;
use flyff_rules::combat::x_random_range;

fn defined(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32, args: Vec<String>) {
    notice::defined(ctx, who, tid, args);
}

fn emit(ctx: &GameCtx<'_>, who: PlayerRef, ev: AuditEvent) {
    let ev = match crate::audit::actor_of(ctx, who) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
}

fn character_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    ctx.player(who)?.player().map(|p| p.ident.character_id)
}

/// The wallet in penya (Perins do not count: the legacy `GetGold`).
fn penya(ctx: &GameCtx<'_>, who: PlayerRef) -> i64 {
    ctx.player(who).map_or(0, crate::item::gold::core::gold)
}

fn vec3_of(v: &pb::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

// ---- buy --------------------------------------------------------------------------------------------------------------

/// `GuildHouseBuy` (22 §12.2, `IsBuyAble`, `ReqBuyGuildHouse`).
pub fn on_buy(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(m) = ctx.player(who) else {
        return;
    };
    // VR-012 / PL-074 / PL-107: the sale NPC within `gameplay.npc_menu_range` of the buyer, in his layer
    if ctx
        .worlds
        .close_npc(MMI_GUILDHOUSE_SALE, m.core.world, m.core.layer, m.core.pos)
        .is_none()
    {
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let Some(g) = svc.social.guild.guild_record_of(c) else {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_BUY_MASTER, Vec::new());
    };
    if g.master() != Some(c) {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_BUY_MASTER, Vec::new());
    }
    if g.level < BUY_MIN_LEVEL {
        return defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_BUY_LEVEL,
            vec![BUY_MIN_LEVEL.to_string()],
        );
    }
    if g.members.len() < BUY_MIN_MEMBERSIZE {
        return defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_BUY_POPULATION,
            vec![BUY_MIN_MEMBERSIZE.to_string()],
        );
    }
    if penya(ctx, who) < BUY_PENYA {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_BUY_EXPENSE, Vec::new());
    }
    let guild = g.id;
    if svc.event.guild_houses.has_house(guild) {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_BUY_HAVE, Vec::new());
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a guild house purchase of a player without a savable session was refused");
        return;
    }
    // ---- the commit block ----
    crate::item::gold::add_gold(svc, ctx, who, -BUY_PENYA, true);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        crate::item::gold::add_gold(svc, ctx, who, BUY_PENYA, true);
        tracing::error!(
            guild = guild.0,
            "a guild house purchase lost its snapshot after the check: the penya was given back"
        );
        return;
    };
    let now = crate::social::party::now_secs(ctx);
    let (house, teleporter) = new_house(svc, guild, now);
    let batch = AtomicBatch::guild_house_buy(snap, house.record(), teleporter.record(guild));
    let upkeep = house.upkeep_until.0;
    svc.event.guild_houses.insert(house);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        log_result("GuildHouseBuy"),
    );
    announce_purchase(svc, ctx, guild);
    emit(
        ctx,
        who,
        audit::guild_house_buy(guild, c, BUY_PENYA, upkeep),
    );
    tracing::info!(guild = guild.0, master = c.0, "guild house: bought");
}

/// The house of a fresh purchase or grant: upkeep for 7 days and the listed AND installed teleporter at its point
/// (keep 7 days, `GuildHouseDBCtrl.cpp:108-143`), and the teleporter entry again for the batch.
pub(crate) fn new_house(
    svc: &mut Services,
    guild: GuildId,
    now: UnixTime,
) -> (GuildHouse, GhFurniture) {
    let until = UnixTime(now.0 + BUY_UPKEEP_DAYS * UPKEEP_DAY_TIME);
    let seq = svc.event.guild_houses.alloc_seq();
    let mut teleporter = GhFurniture::listed(seq, II_GHOU_FUR_NPC_TELEPORTER, now);
    teleporter.installed = true;
    teleporter.pos = TELEPORTER_POS;
    teleporter.angle = 0.0;
    teleporter.keep_until = until;
    teleporter.installed_at = Some(now);
    let house = GuildHouse {
        guild,
        world: WORLD,
        upkeep_until: until,
        created_at: now,
        furniture: vec![teleporter.clone()],
    };
    (house, teleporter)
}

/// `OnBuyGuildHouse`: every online member gets `GuildHouseAllInfo`, `PURCHASE` and the furniture DSTs.
pub(crate) fn announce_purchase(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId) {
    for obj in svc.social.guild.online_objs(&svc.presence, guild) {
        let msg = all_info(svc, ctx, Some(guild));
        crate::social::guild::wire::to_one(ctx, obj, msg);
        notice::defined(
            ctx,
            PlayerRef(obj),
            TID_GAME_GUILDHOUSE_PURCHASE,
            Vec::new(),
        );
        apply_house(svc, ctx, guild, obj, true);
    }
}

// ---- enter, go out ------------------------------------------------------------------------------------------------------

/// `GuildHouseEnter`.
pub fn on_enter(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    enter(svc, ctx, who, false);
}

/// `GuildHouseGoOut` (not NPC-gated): a player in the room of his own guild goes to his marking position.
pub fn on_go_out(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    if let Some(guild) = svc.social.guild.guild_of(c) {
        go_out(ctx, guild, who.0);
    }
}

/// `EnteranceGuildHouse` (`comeback`: the scroll, else the NPC menu 149): no house -> `ENTER_HAVE` (checked first,
/// KEEP); the NPC path needs menu 149 in range and marks the player's place, the scroll path works in Madrigal only and
/// marks `(6968, 0, 3328.8)`; an expired house -> `EXPIRATION`; then the room is made ready, the rest tick starts and
/// the player is replaced to a random point of the enter area (`x` drawn first, then `z`, from the event stream).
/// `true` when the replace was queued (the scroll is consumed then, not before).
pub(crate) fn enter(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    comeback: bool,
) -> bool {
    let Some(c) = character_of(ctx, who) else {
        return false;
    };
    let Some(guild) = svc
        .social
        .guild
        .guild_of(c)
        .filter(|g| svc.event.guild_houses.has_house(*g))
    else {
        defined(ctx, who, TID_GAME_GUILDHOUSE_ENTER_HAVE, Vec::new());
        return false;
    };
    let Some(m) = ctx.player(who) else {
        return false;
    };
    let (world, layer, pos) = (m.core.world, m.core.layer, m.core.pos);
    let mark = if comeback {
        if world != WorldId(WI_WORLD_MADRIGAL) {
            defined(ctx, who, TID_GAME_GUILDHOUSE_COMEBACK_ERROR01, Vec::new());
            return false;
        }
        (WorldId(WI_WORLD_MADRIGAL), COMEBACK_POS)
    } else {
        if ctx
            .worlds
            .close_npc(MMI_GUILDHOUSE_ENTER, world, layer, pos)
            .is_none()
        {
            return false;
        }
        (world, pos)
    };
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.misc.marking_world = mark.0;
        p.misc.marking_pos = mark.1;
    }
    let now = crate::social::party::now_secs(ctx);
    let enterable = svc
        .event
        .guild_houses
        .house(guild)
        .is_some_and(|h| h.upkeep_until.0 > now.0);
    if !enterable {
        defined(ctx, who, TID_GAME_GUILDHOUSE_EXPIRATION, Vec::new());
        return false;
    }
    let Some(layer) = layer_of(guild) else {
        tracing::warn!(
            guild = guild.0,
            "guild house: the guild id gets no room, the entry is refused"
        );
        return false;
    };
    if !ensure_room(svc, ctx, guild) {
        return false;
    }
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.misc.rest_tick = GameMs(ctx.clock.game_ms.0.saturating_add(REST_POINT_TICK_MS));
    }
    let x = x_random_range(ctx.rng.event(), ENTER_X.0, ENTER_X.1) as f32;
    let z = x_random_range(ctx.rng.event(), ENTER_Z.0, ENTER_Z.1) as f32;
    match ctx.worlds.replace(
        who.0,
        WORLD,
        Vec3::new(x, ENTER_Y, z),
        layer,
        ReplaceKind::Normal,
    ) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(player = who.0.0, error = %e, "guild house: the entry was refused");
            check_destroy_room(svc, ctx, guild, None);
            false
        }
    }
}

// ---- the item rows ------------------------------------------------------------------------------------------------------

/// The IK2 rows 38, 39, 40 (list up) and 41 (the comeback scroll): they need the services, so the row queues
/// [`Followup::GuildHouseItem`] and reports itself done (`Handled`: the follow-up consumes the item).
pub fn use_row(m: &mut Mover, prop: &ItemProp, ux: &mut RowCx<'_>, fx: &mut Fx) -> RowResult {
    fx.then(Followup::GuildHouseItem {
        player: m.id(),
        slot: ux.id,
        item_id: prop.id,
    });
    RowResult::Handled
}

/// The follow-up of [`use_row`]: the slot must still hold the item (else silent); the kinds 38, 39 and 40 list the item
/// up, 41 enters the house and is consumed (`RemoveReason::Consume`, then the honor counter) only when the entry worked.
pub fn on_item_followup(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    slot: StorageId,
    item_id: u32,
) {
    let holds = ctx
        .player(who)
        .and_then(|m| m.player())
        .and_then(|p| p.items.inventory.get_at_id(slot))
        .is_some_and(|it| it.item_id == item_id);
    if !holds {
        return;
    }
    let Some(kind2) = ctx.data.item(item_id).map(|p| p.item_kind2) else {
        return;
    };
    match kind2 {
        IK2_GUILDHOUSE_FURNITURE | IK2_GUILDHOUSE_NPC | IK2_GUILDHOUSE_PAPERING => {
            list_up(svc, ctx, who, slot, item_id);
        }
        IK2_GUILDHOUES_COMEBACK if enter(svc, ctx, who, true) => {
            remove_item(svc, ctx, who, slot, 1, RemoveReason::Consume);
            crate::social::hooks::on_item_used(svc, ctx, who, item_id);
        }
        _ => {}
    }
}

/// List-up (22 §12.3, `IsAuthority` then `IsListUpAble`): no house -> silent; no `GH_FURNITURE` power ->
/// `INSIDE_INSTALL_LEVEL`; not in the room of the own guild -> silent; the list at 200 entries or 200 of the item's kind3
/// -> `INSIDE_SPACE`. Then ONE `GuildHouseFurnitureAdd` batch with the player's snapshot (the item gone).
fn list_up(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    slot: StorageId,
    item_id: u32,
) {
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let Some(guild) = svc
        .social
        .guild
        .guild_of(c)
        .filter(|g| svc.event.guild_houses.has_house(*g))
    else {
        return;
    };
    if !svc
        .social
        .guild
        .has_power(c, crate::social::guild::GuildPower::GH_FURNITURE)
    {
        return defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_INSIDE_INSTALL_LEVEL,
            Vec::new(),
        );
    }
    if !is_inside(ctx, who.0, guild) {
        return;
    }
    let Some(prop) = ctx.data.item(item_id) else {
        return;
    };
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let same_kind = house
        .furniture
        .iter()
        .filter(|f| {
            ctx.data
                .item(f.item)
                .is_some_and(|p| p.item_kind3 == prop.item_kind3)
        })
        .count();
    if house.furniture.len() >= MAX_LISTUP_NUM || same_kind >= MAX_LISTUP_NUM {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_INSIDE_SPACE, Vec::new());
    }
    if !can_snapshot(ctx, who) {
        return;
    }
    // ---- the commit block ----
    remove_item(svc, ctx, who, slot, 1, RemoveReason::Moved);
    crate::social::hooks::on_item_used(svc, ctx, who, item_id);
    let now = crate::social::party::now_secs(ctx);
    let seq = svc.event.guild_houses.alloc_seq();
    let entry = GhFurniture::listed(seq, item_id, now);
    let record = entry.record(guild);
    let index = {
        let Some(house) = svc.event.guild_houses.house_mut(guild) else {
            return;
        };
        house.furniture.push(entry);
        house.furniture.len() - 1
    };
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        tracing::error!(
            guild = guild.0,
            item = item_id,
            "a furniture list-up lost its snapshot after the check: the entry is kept by the memory only"
        );
        return;
    };
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        AtomicBatch::guild_house_furniture_add(snap, record),
        log_result("GuildHouseFurnitureAdd"),
    );
    if let Some(house) = svc.event.guild_houses.house(guild)
        && let Some(f) = house.furniture.get(index)
    {
        let info = fntr_info(ctx.worlds, guild, f);
        super::notify(
            svc,
            ctx,
            guild,
            GhOp::ListUp,
            i32::try_from(index).unwrap_or(i32::MAX),
            info,
        );
    }
    emit(
        ctx,
        who,
        audit::guild_house_furniture_add(guild, c, item_id, seq),
    );
}

// ---- GuildHousePacketReq ------------------------------------------------------------------------------------------------

/// `GuildHousePacketReq{op, index, info}` (22 §12.4): SETUP, REMOVE, RESET and UPKEEP; every other op, an unknown op and
/// a player without a house are ignored silently.
pub fn on_packet_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::GuildHousePacketReq,
) {
    let Some(op) = GhOp::from_i32(req.op) else {
        return;
    };
    if !matches!(op, GhOp::Setup | GhOp::Remove | GhOp::Reset | GhOp::Upkeep) {
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    let Some(guild) = svc
        .social
        .guild
        .guild_of(c)
        .filter(|g| svc.event.guild_houses.has_house(*g))
    else {
        return;
    };
    if op == GhOp::Upkeep {
        return upkeep(svc, ctx, who, c, guild, &req);
    }
    if !svc
        .social
        .guild
        .has_power(c, crate::social::guild::GuildPower::GH_FURNITURE)
    {
        return defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_INSIDE_INSTALL_LEVEL,
            Vec::new(),
        );
    }
    if !is_inside(ctx, who.0, guild) || !super::guild_bank_on(ctx) {
        return;
    }
    // VR-004: the index of the server's list
    let len = svc
        .event
        .guild_houses
        .house(guild)
        .map_or(0, |h| h.furniture.len());
    let Some(index) = usize::try_from(req.index).ok().filter(|i| *i < len) else {
        return;
    };
    let placement = match op {
        GhOp::Setup | GhOp::Reset => {
            let Some(info) = req.info.as_ref() else {
                return;
            };
            let Some(pos) = info.pos.as_ref().map(vec3_of) else {
                return;
            };
            let inside = pos.is_finite()
                && info.angle.is_finite()
                && ctx
                    .worlds
                    .world(WORLD)
                    .is_some_and(|w| w.statics.in_world3(pos));
            if !inside {
                return;
            }
            Some((pos, info.angle))
        }
        _ => None,
    };
    match op {
        GhOp::Setup => setup(svc, ctx, who, guild, index, placement),
        GhOp::Remove => remove(svc, ctx, guild, index),
        GhOp::Reset => reset(svc, ctx, guild, index, placement),
        _ => {}
    }
}

/// The wire entry of the entry at `index`, then the persist op and the notification of every successful op.
fn finish_op(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId, index: usize, op: GhOp) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let Some(f) = house.furniture.get(index) else {
        return;
    };
    ctx.persist
        .op(DbOp::UpsertGuildHouseFurniture(f.record(guild)));
    let info = fntr_info(ctx.worlds, guild, f);
    super::notify(
        svc,
        ctx,
        guild,
        op,
        i32::try_from(index).unwrap_or(i32::MAX),
        info,
    );
}

/// SETUP: the entry is not installed; `ability_max > 1` -> `FUR_INSTALL_LEVEL [ability_max]`; as many installed entries
/// of its kind3 as the maximum (chair 4, else 1) -> `INSIDE_INSTALL_KIND [max]`; the teleporter is forced to its point;
/// the keep time starts at the first setup; the object is spawned and the DSTs go to every online member.
fn setup(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    guild: GuildId,
    index: usize,
    placement: Option<(Vec3, f32)>,
) {
    let Some((client_pos, client_angle)) = placement else {
        return;
    };
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let Some(entry) = house.furniture.get(index) else {
        return;
    };
    if entry.installed {
        return;
    }
    let Some(prop) = ctx.data.item(entry.item) else {
        return;
    };
    if prop.ability_max > 1 {
        return defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_FUR_INSTALL_LEVEL,
            vec![prop.ability_max.to_string()],
        );
    }
    let max = if prop.item_kind3 == IK3_CHAIR { 4 } else { 1 };
    let same = house
        .installed()
        .filter(|(_, f)| {
            ctx.data
                .item(f.item)
                .is_some_and(|p| p.item_kind3 == prop.item_kind3)
        })
        .count();
    if same >= max {
        return defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_INSIDE_INSTALL_KIND,
            vec![max.to_string()],
        );
    }
    let teleporter = prop.item_kind3 == IK3_TELEPORTER;
    let now = crate::social::party::now_secs(ctx);
    let keep = if entry.keep_until.0 == 0 {
        UnixTime(now.0 + i64::from(prop.ability_min) * 60)
    } else {
        entry.keep_until
    };
    // ---- the commit block ----
    let Some(house) = svc.event.guild_houses.house_mut(guild) else {
        return;
    };
    let Some(f) = house.furniture.get_mut(index) else {
        return;
    };
    f.installed = true;
    (f.pos, f.angle) = if teleporter {
        (TELEPORTER_POS, 0.0)
    } else {
        (client_pos, client_angle)
    };
    f.keep_until = keep;
    f.installed_at = Some(now);
    let snapshot = f.clone();
    let obj = if room_live(ctx.worlds, guild) {
        spawn_object(ctx, guild, &snapshot)
    } else {
        None
    };
    if let Some(f) = svc
        .event
        .guild_houses
        .house_mut(guild)
        .and_then(|h| h.furniture.get_mut(index))
    {
        f.obj = obj;
    }
    apply_entry_to_members(svc, ctx, guild, &snapshot);
    finish_op(svc, ctx, guild, index, GhOp::Setup);
}

/// REMOVE: the entry is installed; the object goes, the DSTs are reset for every online member, position, angle and
/// keep time stay.
fn remove(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId, index: usize) {
    let Some(f) = svc
        .event
        .guild_houses
        .house(guild)
        .and_then(|h| h.furniture.get(index))
        .filter(|f| f.installed)
    else {
        return;
    };
    let (src, obj) = (source_of(f), f.obj);
    // ---- the commit block ----
    if let Some(o) = obj {
        ctx.worlds.delete(o);
    }
    if let Some(f) = svc
        .event
        .guild_houses
        .house_mut(guild)
        .and_then(|h| h.furniture.get_mut(index))
    {
        f.installed = false;
        f.obj = None;
    }
    reset_entry_for_members(svc, ctx, guild, src);
    finish_op(svc, ctx, guild, index, GhOp::Remove);
}

/// RESET: an installed entry that is not the teleporter moves (delete + respawn at the client's point), the keep time
/// is unchanged. The teleporter is refused before anything is written (FIX: the original wrote the database first).
fn reset(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    index: usize,
    placement: Option<(Vec3, f32)>,
) {
    let Some((pos, angle)) = placement else {
        return;
    };
    let Some(f) = svc
        .event
        .guild_houses
        .house(guild)
        .and_then(|h| h.furniture.get(index))
        .filter(|f| f.installed)
    else {
        return;
    };
    if ctx
        .data
        .item(f.item)
        .is_none_or(|p| p.item_kind3 == IK3_TELEPORTER)
    {
        return;
    }
    let old = f.obj;
    // ---- the commit block ----
    if let Some(o) = old {
        ctx.worlds.delete(o);
    }
    let Some(f) = svc
        .event
        .guild_houses
        .house_mut(guild)
        .and_then(|h| h.furniture.get_mut(index))
    else {
        return;
    };
    f.pos = pos;
    f.angle = angle;
    let snapshot = f.clone();
    let obj = if room_live(ctx.worlds, guild) {
        spawn_object(ctx, guild, &snapshot)
    } else {
        None
    };
    if let Some(f) = svc
        .event
        .guild_houses
        .house_mut(guild)
        .and_then(|h| h.furniture.get_mut(index))
    {
        f.obj = obj;
    }
    finish_op(svc, ctx, guild, index, GhOp::Reset);
}

// ---- UPKEEP -------------------------------------------------------------------------------------------------------------

/// UPKEEP (22 §12.4): `days = info.item_id`, checked before any arithmetic (VR-066): 0 days or 35 and more ->
/// `TAX_MAX [35]` (FIX: the original accepted 0 days at no cost while days remained, and its `days * 86 400` wrapped in
/// 32 bits). In `i64`: `new = days * 86 400 + (upkeep > 0 ? upkeep : now)`, `total = (new - now) / 86 400` must be in
/// `1..35`, the cost `days * 2 400 000` must be in the wallet (`BUY_EXPENSE`). ONE `GuildHouseUpkeep` batch.
fn upkeep(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    c: CharacterId,
    guild: GuildId,
    req: &pb::GuildHousePacketReq,
) {
    if !svc
        .social
        .guild
        .has_power(c, crate::social::guild::GuildPower::GH_UPKEEP)
    {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_TAX_LEVEL, Vec::new());
    }
    let Some(info) = req.info.as_ref() else {
        return;
    };
    let days = i64::from(info.item_id);
    let tax_max = |ctx: &mut GameCtx<'_>| {
        defined(
            ctx,
            who,
            TID_GAME_GUILDHOUSE_TAX_MAX,
            vec![MAX_UPKEEP_DAY.to_string()],
        );
    };
    if days == 0 || days >= MAX_UPKEEP_DAY {
        return tax_max(ctx);
    }
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let now = crate::social::party::now_secs(ctx);
    let base = if house.upkeep_until.0 > 0 {
        house.upkeep_until.0
    } else {
        now.0
    };
    let new = days * UPKEEP_DAY_TIME + base;
    let total = (new - now.0) / UPKEEP_DAY_TIME;
    if total <= 0 || total >= MAX_UPKEEP_DAY {
        return tax_max(ctx);
    }
    let cost = days * UPKEEP_DAY_PENYA_SMALL;
    if cost > penya(ctx, who) {
        return defined(ctx, who, TID_GAME_GUILDHOUSE_BUY_EXPENSE, Vec::new());
    }
    if !can_snapshot(ctx, who) {
        return;
    }
    // ---- the commit block ----
    crate::item::gold::add_gold(svc, ctx, who, -cost, true);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        crate::item::gold::add_gold(svc, ctx, who, cost, true);
        tracing::error!(
            guild = guild.0,
            "a guild house upkeep lost its snapshot after the check: the penya was given back"
        );
        return;
    };
    let Some(house) = svc.event.guild_houses.house_mut(guild) else {
        return;
    };
    house.upkeep_until = UnixTime(new);
    let record = house.record();
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        AtomicBatch::guild_house_upkeep(snap, record),
        log_result("GuildHouseUpkeep"),
    );
    super::notify(
        svc,
        ctx,
        guild,
        GhOp::Upkeep,
        req.index,
        upkeep_info(info.item_id, new),
    );
    emit(
        ctx,
        who,
        audit::guild_house_upkeep(guild, c, cost, u32::try_from(days).unwrap_or(u32::MAX), new),
    );
}

/// The `GuildHousePacketNtf` info of an UPKEEP: the days in `item_id`, the new upkeep time in `expire_at`.
pub(crate) fn upkeep_info(days: u32, until: i64) -> pb::GhFntrInfo {
    pb::GhFntrInfo {
        item_id: days,
        setup: false,
        pos: Some(crate::movement::intents::pb_vec(Vec3::ZERO)),
        angle: 0.0,
        ctrl_obj_id: super::NO_OBJ,
        expire_at: until,
    }
}
