//! The inventory handlers (Plan/19 §5.13-§5.14, WP-ITEM-04): `MoveItemReq`, `DoEquipReq`, `RemoveInvenItem` and
//! `DropItem`.
//!
//! Every handler resolves the actor from the session (VR-001) and checks the item it names against the
//! actor's own inventory: a foreign, missing, stale or reserved `obj_id` is ignored and changes nothing
//! (VR-010). Equip decisions belong to `stats::equip` (doc 16): the handler plans, and on `Ok(plan)` commits
//! exactly once; its own side effects (the equip-bind flag, the deletion of a vagrant ride, the pet dismissal)
//! run around the commit.

use super::container::StorageId;
use super::env::ItemEnv;
use super::fx::Fx;
use super::loot::GroundItemMeta;
use super::model::Item;
use super::ops::{RemoveReason, core as ops};
use super::usage::cast::{self, CastStep};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::equip::{self, EquipFacts};
use crate::world::{ItemDrop, terrain::HeightAttr};
use flyff_common::config::EventId;
use flyff_common::defs::item::{II_CHP_RED, II_SYS_SYS_SCR_SEALCHARACTER};
use flyff_common::defs::itemkind::{IK3_EGG, IK3_QUEST};
use flyff_common::defs::text::{
    TID_GAME_CANNOT_DO_USINGITEM, TID_GAME_FLIGHTDROP, TID_GAME_SUCCESS_REMOVE_ITEM,
    TID_GAME_TRADELIMITITEM,
};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_data::props::item::ItemProp;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::consts::{ITEM_MODE, MAX_HUMAN_PARTS};
use flyff_rules::item::{IP_FLAG_EQUIP_BIND, IP_FLAG_UNDESTRUCTABLE, ITEM_FLAG_BINDS};
use serde_json::json;

/// A drop lands at most this far (XZ, metres) from the player (VR-028, PL-078).
pub const DROP_MAX_DISTANCE: f32 = 3.0;

/// `m_nFlightLv`: derived from the level (16 §0.13: `level >= 20 ? 1 : 0`).
fn flight_level(level: i32) -> i32 {
    i32::from(level >= 20)
}

/// The world and player facts the equip planner reads (ride rules).
pub fn equip_facts(ctx: &GameCtx<'_>, who: PlayerRef) -> EquipFacts {
    let mut f = EquipFacts::default();
    let Some(m) = ctx.player(who) else { return f };
    f.flight_level = flight_level(m.mv.level);
    if let Some(s) = ctx.worlds.statics_of(who.0) {
        f.fly_allowed = s.settings.fly;
        f.terrain = s.height_attr(m.core.pos.x, m.core.pos.z);
    } else {
        f.terrain = HeightAttr::None;
    }
    f
}

/// `MoveItemReq`: swaps two bag slots (19 §5.13.1). Both slots must be bag slots and differ, the destination
/// empty or usable, the source an existing usable item; the answer is `MoveItemNtf` to the sender.
pub fn on_move_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::MoveItemReq,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        move_item(m, &env, msg.src_slot, msg.dst_slot, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// The core of [`on_move_item`].
pub fn move_item(m: &mut Mover, env: &ItemEnv<'_>, src: u32, dst: u32, fx: &mut Fx) {
    let Some(p) = m.player_mut() else { return };
    let inv = &mut p.items.inventory;
    let bag = u32::from(inv.index_num());
    if src == dst || src >= bag || dst >= bag {
        return;
    }
    let (src, dst) = (src as u16, dst as u16);
    let dst_ok = inv.get_at_slot(dst).is_none_or(Item::is_usable);
    let src_ok = inv.get_at_slot(src).is_some_and(Item::is_usable);
    if dst_ok && src_ok {
        inv.swap(src, dst, env.data());
        fx.out.to_self(pb::MoveItemNtf {
            item_type: 0,
            src_slot: u32::from(src),
            dst_slot: u32::from(dst),
        });
    }
}

/// `RemoveInvenItem`: destroys items from the bag (19 §5.13.3).
pub fn on_remove_inven_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::RemoveInvenItem,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        remove_inven_item(m, &env, msg.obj_id, msg.num, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// The core of [`on_remove_inven_item`]: `num <= 0`, a missing or reserved or worn item, a count above the
/// stack and an undestructable prop are ignored; an item in use gets `TID_GAME_CANNOT_DO_USINGITEM`.
pub fn remove_inven_item(m: &mut Mover, env: &ItemEnv<'_>, obj_id: u32, num: i32, fx: &mut Fx) {
    if num <= 0 {
        return;
    }
    let Ok(id) = StorageId::try_from(obj_id) else {
        return;
    };
    let Some(p) = m.player() else { return };
    let inv = &p.items.inventory;
    let Some(it) = inv.get_at_id(id) else { return };
    if !it.is_usable() || inv.is_equip(id) {
        return;
    }
    if p.items.is_using(id) {
        fx.text(TID_GAME_CANNOT_DO_USINGITEM);
        return;
    }
    if i32::from(it.count) < num {
        return;
    }
    let Some(prop) = env.data().item(it.item_id) else {
        return;
    };
    if prop.flag & IP_FLAG_UNDESTRUCTABLE != 0 {
        return;
    }
    fx.defined(
        TID_GAME_SUCCESS_REMOVE_ITEM,
        vec![prop.name.clone(), num.to_string()],
    );
    ops::remove_item(m, env, id, num as u16, RemoveReason::Delete, fx);
}

/// `DoEquipReq`: equip or unequip (19 §5.13.2). A `part` of 31 or more, a missing or reserved item and a
/// dead actor are ignored; a worn item can only be named with its own part.
pub fn on_do_equip(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::DoEquipReq) {
    let Ok(id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    equip_toggle(svc, ctx, who, id, msg.part);
}

/// The equip toggle shared by `DoEquipReq` and the equipment branch of `DoUseItem` (19 §5.1 step 5).
/// `part`: -1 / 0 = automatic, `1..31` = the part of a worn item to take off.
pub fn equip_toggle(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    id: StorageId,
    part: i32,
) {
    if usize::try_from(part).is_ok_and(|p| p >= MAX_HUMAN_PARTS) {
        return;
    }
    let facts = equip_facts(ctx, who);
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now = ctx.clock.game_ms;
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        toggle_equip(m, &env, id, part, &facts, now, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// The core of [`equip_toggle`]: validates the item (VR-010), plans, commits once and runs the handler's own
/// side effects. Returns `true` when an equip or unequip happened.
pub fn toggle_equip(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    part: i32,
    facts: &EquipFacts,
    now: flyff_common::GameMs,
    fx: &mut Fx,
) -> bool {
    let Some(p) = m.player() else { return false };
    if m.sim.act.state.is_die() {
        return false;
    }
    let inv = &p.items.inventory;
    let Some(item) = inv.get_at_id(id) else {
        return false;
    };
    if !item.is_usable() {
        return false;
    }
    let worn = inv.is_equip(id);
    if part > 0 {
        if worn && inv.equip_id(part as u32) != Some(id) {
            return false;
        }
    } else if worn {
        return false;
    }
    let Some(prop) = env.data().item(item.item_id) else {
        return false;
    };
    let item_part = if worn {
        inv.slot_of(id)
            .map(|s| u32::from(s) - u32::from(inv.index_num()))
    } else {
        None
    };
    // an item with a cast time is equipped after the cast (19 §5.2); taking it off needs none
    if item_part.is_none() {
        match cast::ready_check(m, env, id, prop, now, fx) {
            CastStep::Proceed => {}
            CastStep::Deferred | CastStep::Refused => return false,
        }
    }
    let plan = match item_part {
        Some(part) => equip::plan_unequip(m, env, part, facts),
        None => equip::plan_equip(m, env, id, facts),
    };
    let plan = match plan {
        Ok(plan) => plan,
        Err(denied) => {
            if let Some(tid) = denied.tid {
                fx.defined(tid, denied.args);
            }
            return false;
        }
    };
    let vagrant_ride = Item::is_vagrant_ride(prop);
    let bind = prop.flag & IP_FLAG_EQUIP_BIND != 0;
    let ride_equip = plan.equip && prop.parts == flyff_common::defs::neuz::PARTS_RIDE;
    if ride_equip && env.cfg().improved_pets {
        dismiss_pets(m, env, now, fx);
    }
    let report = equip::commit(m, env, &plan, &mut fx.out);
    if report.ride_equipped || report.ride_unequipped || report.weapon_unequipped {
        tracing::trace!(
            ?report,
            "equip: the sim messages of the commit are left to the caller"
        );
    }
    if plan.equip {
        // IP_FLAG_EQUIP_BIND: the item binds when first worn
        let flags = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(id))
            .map(|it| it.flags.0);
        if bind
            && let Some(flags) = flags
            && flags & ITEM_FLAG_BINDS == 0
        {
            ops::set_item_flags(m, env, id, flags | ITEM_FLAG_BINDS, fx);
        }
    } else if vagrant_ride {
        // a vagrant-job ride is deleted when taken off
        let count = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(id))
            .map_or(0, |it| it.count);
        ops::remove_item(m, env, id, count, RemoveReason::Other("vagrant_ride"), fx);
    }
    true
}

/// Dismisses the summoned system pet and eat pet when a ride is equipped (`features.improved_pets`): the system
/// pet is released with its messages (ITEM-16), the eat pet's handle is cleared (its mover is WP-ITEM-18's).
fn dismiss_pets(m: &mut Mover, env: &ItemEnv<'_>, now: flyff_common::GameMs, fx: &mut Fx) {
    crate::item::pets::system::release(m, &crate::buff::BuffCx::new(*env, now), fx);
    if let Some(p) = m.player_mut() {
        p.items.summoned_eat_pet = None;
    }
}

/// `IsDropable(item)` (src:_Common/Mover.cpp:9512-9533): `Ok` or the refusal (`Some(tid)` = a text for the
/// player, `None` = silent).
pub fn is_dropable(
    m: &Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    prop: &ItemProp,
    flying_or_airborne: bool,
    trading: bool,
) -> Result<(), Option<u32>> {
    let Some(p) = m.player() else {
        return Err(None);
    };
    if flying_or_airborne {
        return Err(Some(TID_GAME_FLIGHTDROP));
    }
    if trading {
        return Err(Some(TID_GAME_TRADELIMITITEM));
    }
    if m.mv.mode & ITEM_MODE != 0 {
        return Err(None);
    }
    let Some(it) = p.items.inventory.get_at_id(id) else {
        return Err(None);
    };
    let bad = !it.is_usable()
        || it.is_binds(prop, env.cfg().only_armor_awakes)
        || it.is_guild_cloak(prop)
        || p.items.inventory.is_equip(id)
        || p.items.is_using(id)
        || Item::is_vagrant_ride(prop)
        || it.is_charged()
        || prop.item_kind3 == IK3_EGG
        || it.item_id == II_CHP_RED
        || it.item_id == II_SYS_SYS_SCR_SEALCHARACTER;
    if bad { Err(None) } else { Ok(()) }
}

/// Clamps the client's drop position to `max` metres from the player in the XZ plane (VR-028); the height is
/// the player's.
pub fn clamp_drop_pos(player: Vec3, wanted: Vec3, max: f32) -> Vec3 {
    let (dx, dz) = (wanted.x - player.x, wanted.z - player.z);
    let d2 = dx * dx + dz * dz;
    if !d2.is_finite() {
        return player;
    }
    if d2 <= max * max {
        return Vec3::new(wanted.x, player.y, wanted.z);
    }
    let k = max / d2.sqrt();
    Vec3::new(player.x + dx * k, player.y, player.z + dz * k)
}

/// What `on_drop_item` needs from the worlds, copied out so that no borrow outlives the checks.
struct DropSnap {
    item: Item,
    pos: Vec3,
    world: flyff_common::WorldId,
    layer: flyff_common::LayerId,
    character: flyff_common::CharacterId,
    verdict: Result<(), Option<u32>>,
}

/// `DropItem` (19 §5.14, VR-028, PL-078, KQ-16): `num <= 0` is ignored and `num` clamped to the stack; the
/// item must be droppable; with `events.drop_item_remove` (shipped) the items are destroyed, else a ground
/// item appears at the clamped position (at most 3 m from the player) and the stack shrinks.
pub fn on_drop_item(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::DropItem) {
    if msg.num <= 0 {
        return;
    }
    let Ok(id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let remove_mode = ctx.live.event_state(EventId::DropItemRemove) != 0;
    let snap = {
        let Some(m) = ctx.worlds.player(who.0) else {
            return;
        };
        let Some(p) = m.player() else { return };
        let Some(item) = p.items.inventory.get_at_id(id) else {
            return;
        };
        if ctx.worlds.world(m.core.world).is_none() {
            return;
        }
        let Some(prop) = env.data().item(item.item_id) else {
            return;
        };
        let airborne = m.sim.act.ground == 0 || m.sim.act.flags.fly();
        // the trade and vendor states of the economy (ECON-03/04) do not exist yet: no partner, no stall
        DropSnap {
            item: item.clone(),
            pos: m.core.pos,
            world: m.core.world,
            layer: m.core.layer,
            character: p.ident.character_id,
            verdict: is_dropable(m, &env, id, prop, airborne, false),
        }
    };
    if let Err(tid) = snap.verdict {
        if let Some(tid) = tid {
            let mut fx = Fx::new();
            fx.text(tid);
            fx.flush(svc, ctx, who);
        }
        return;
    }
    let Some(prop) = env.data().item(snap.item.item_id) else {
        return;
    };
    let num = msg.num.min(i32::from(snap.item.count)).max(1) as u16;
    let mut fx = Fx::new();
    if remove_mode {
        // KQ-16: the item is destroyed, nothing appears on the ground
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            ops::remove_item(m, &env, id, num, RemoveReason::DropRemove, &mut fx);
        }
        fx.flush(svc, ctx, who);
        return;
    }
    // a ground item: the whole stack keeps its serial, a split part gets a fresh one (19 §2.7)
    let mut dropped = snap.item.clone();
    dropped.count = num;
    dropped.reserved = 0;
    if num != snap.item.count {
        let mut serials = super::ops::CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
        match super::container::SerialSource::next_serial(&mut serials) {
            Ok(s) => dropped.serial = flyff_common::ItemSerial(s),
            Err(e) => {
                tracing::warn!(error = %e, "drop: no serial for a split stack");
                return;
            }
        }
    }
    let wanted = msg
        .pos
        .as_ref()
        .map_or(snap.pos, |v| Vec3::new(v.x, v.y, v.z));
    let pos = clamp_drop_pos(snap.pos, wanted, DROP_MAX_DISTANCE);
    let angle = ctx.rng.world(snap.world).random_below(360) as f32;
    let meta = GroundItemMeta {
        owner: None,
        holder: (prop.item_kind3 == IK3_QUEST).then_some(snap.character),
        drop_time: ctx.clock.game_ms,
        from_monster: false,
        event_monster: None,
        gold: None,
        respawned: false,
    };
    let drop = ItemDrop::new(dropped.to_record(0), pos, angle, meta);
    if ctx.worlds.spawn_item(snap.world, snap.layer, drop).is_err() {
        return;
    }
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        ops::remove_item(m, &env, id, num, RemoveReason::Moved, &mut fx);
    }
    fx.audit(
        ev::ITEM_DROP,
        json!({ "item": super::fx::item_json(&dropped) }),
    );
    fx.flush(svc, ctx, who);
}
