//! The requests of the mini room: the list-up by item use, setup and removal of furniture, the visitor list, the visit
//! and the way out, and the messenger and deletion seams (`CHousingMng::Req*`, `CDPSrvr::OnHousing*`,
//! `MoverSkill.cpp:1391-1411`; Plan/22 §13.2, VR-012, VR-062).
//!
//! **Silent refusals.** The original answered a refused setup, visit or visitor change with nothing, and so does this
//! module; the only texts are `OVERMAX`, `ADDITION_FAIL` and `INSTALL_FAIL02`.
//!
//! **The list-up** is the one operation that waits for the database: the item is consumed, the entry is listed as
//! `pending` and the `HousingFurnitureAdd` batch (the owner's snapshot with the item gone and the strict insert of the
//! row) is submitted with a ticket. While it is in flight the room ignores a second list-up and every setup (the legacy
//! `m_bSetting`). A failed batch rolled the snapshot back, so the stored inventory still holds the item: the entry is
//! dropped and the item is created again (the legacy `OnHousingDBFailed`; the Plan/19 §20.3 "never compensated" rule
//! does not fit, the item would vanish at the next snapshot).

use super::room::{avatar_of, character_of, go_out, layer_live, put_up, take_down};
use super::{HousingItem, MAX_LIST, ROOM_ENTRY, ROOM_ENTRY_ANGLE, ROOM_WORLD, room_layer};
use crate::chat::ingress::notice;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, create_item_or_mail, snapshot_for_batch, submit};
use crate::follow::Followup;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::ops::{ItemOrigin, RemoveReason, remove_item};
use crate::item::usage::{RowCx, RowResult};
use crate::mover::Mover;
use crate::tickets::TicketResult;
use crate::world::ReplaceKind;
use flyff_common::defs::itemkind::{IK1_HOUSING, IK2_FURNITURE, IK2_PAPERING, IK3_PROPS};
use flyff_common::defs::neuz::MMI_VISIT_FRIEND;
use flyff_common::defs::text::{
    TID_GAME_HOUSING_ADDITION_FAIL, TID_GAME_HOUSING_ENTER, TID_GAME_HOUSING_INSTALL_FAIL02,
    TID_GAME_HOUSING_OVERMAX,
};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, UnixTime};
use flyff_db::{AtomicBatch, DbOp};
use flyff_proto::pb;

// ---- the item use: list-up ---------------------------------------------------------------------------------------

/// The `IK2_FURNITURE` / `IK2_PAPERING` row of the item use (`MoverSkill.cpp:1391`): the pipeline neither consumes nor
/// post-processes, the follow-up [`list_up`] does everything once the services are at hand.
pub fn use_row(m: &Mover, ux: &RowCx<'_>, fx: &mut Fx) -> RowResult {
    fx.then(Followup::HousingListUp {
        player: m.id(),
        slot: ux.id,
    });
    RowResult::Handled
}

/// `CHousingMng::ReqSetFurnitureList` for the housing item at inventory slot `slot` of `p`. See the module doc.
pub fn list_up(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, slot: StorageId) {
    let Some(owner) = character_of(ctx, p) else {
        return;
    };
    // the item must still be there, usable, and a housing item
    let Some(item) = ctx
        .player(p)
        .and_then(|m| m.player())
        .and_then(|pl| pl.items.inventory.get_at_id(slot))
        .filter(|it| it.is_usable())
        .cloned()
    else {
        return;
    };
    let Some(prop) = ctx.data.item(item.item_id) else {
        return;
    };
    if prop.item_kind1 != IK1_HOUSING
        || (prop.item_kind2 != IK2_FURNITURE && prop.item_kind2 != IK2_PAPERING)
    {
        return;
    }
    let (paper, minutes) = (prop.item_kind2 == IK2_PAPERING, prop.ability_min);
    let Some(room) = svc.event.housing.room(owner) else {
        return;
    };
    if room.pending {
        return; // the legacy `m_bSetting`: silent
    }
    if room.items.len() >= MAX_LIST {
        notice::defined(ctx, p, TID_GAME_HOUSING_OVERMAX, Vec::new());
        return;
    }
    if room.index_of(item.item_id).is_some() {
        notice::defined(ctx, p, TID_GAME_HOUSING_ADDITION_FAIL, Vec::new());
        return;
    }
    if !can_snapshot(ctx, p) {
        return;
    }
    // ---- the commit block: nothing below can refuse ----
    let now = ctx.clock.unix_ms.to_unix_time();
    let keep_until = UnixTime(now.0.saturating_add(i64::from(minutes).saturating_mul(60)));
    let item_id = item.item_id;
    // what the failure re-creates: one piece with the serial of the removed one (a stack of several keeps the rest)
    let mut spare = item.clone();
    spare.count = 1;
    if item.count > 1 {
        spare.serial = flyff_common::ItemSerial(0);
    }
    remove_item(svc, ctx, p, slot, 1, RemoveReason::Moved);
    let entry = HousingItem {
        item: item_id,
        keep_until,
        installed: false,
        pos: Vec3::ZERO,
        angle: 0.0,
        listed_at: now,
        obj: None,
        pending: true,
        paper,
    };
    let record = entry.record(owner);
    if let Some(room) = svc.event.housing.rooms_mut().get_mut(&owner) {
        room.items.push(entry);
        room.pending = true;
    }
    // the honor counter of `OnAfterUseItem`
    crate::social::hooks::on_item_used(svc, ctx, p, item_id);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, p) else {
        // unreachable after `can_snapshot`: put everything back
        undo_list_up(svc, ctx, owner, item_id, spare);
        return;
    };
    let batch = AtomicBatch::housing_furniture_add(snap, record);
    submit(
        svc,
        ctx,
        p,
        Some(generation),
        batch,
        move |svc, ctx, result| {
            on_list_up_result(svc, ctx, owner, item_id, keep_until, spare, result)
        },
    );
}

/// The answer of the `HousingFurnitureAdd` batch.
fn on_list_up_result(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    item_id: u32,
    keep_until: UnixTime,
    spare: crate::item::model::Item,
    result: TicketResult,
) {
    match result {
        TicketResult::Committed(_) => {
            // the owner left meanwhile: the row is stored and loads with him. A room that was rebuilt from the load
            // already holds the entry as a settled one and has announced it in `HousingAllInfo`
            let info = svc
                .event
                .housing
                .rooms_mut()
                .get_mut(&owner)
                .and_then(|room| {
                    let info = room
                        .items
                        .iter_mut()
                        .find(|i| i.item == item_id && i.pending)
                        .map(|i| {
                            i.pending = false;
                            i.info()
                        });
                    if info.is_some() {
                        room.pending = false;
                    }
                    info
                });
            if let Some(info) = info {
                super::room::tell_owner(
                    svc,
                    ctx,
                    owner,
                    pb::HousingFurnitureList {
                        info: Some(info),
                        add: true,
                    },
                );
            }
            let mut ev =
                flyff_db::audit::content::housing_furniture_add(owner, item_id, keep_until.0);
            if let Some(a) = avatar_of(svc, ctx, owner)
                && let Some(actor) = crate::audit::actor_of(ctx, PlayerRef(a))
            {
                ev = ev.by(actor);
            }
            ctx.audit.emit(ev);
        }
        other => {
            tracing::error!(
                owner = owner.0,
                item = item_id,
                result = ?other,
                "housing: the list-up batch failed; the item is created again"
            );
            undo_list_up(svc, ctx, owner, item_id, spare);
        }
    }
}

/// A list-up that did not happen: the entry goes, the lock opens and the item comes back (the legacy
/// `OnHousingDBFailed`). An owner who is gone is logged at error with the item so the operator can restore it.
fn undo_list_up(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    item_id: u32,
    spare: crate::item::model::Item,
) {
    if let Some(room) = svc.event.housing.rooms_mut().get_mut(&owner) {
        let before = room.items.len();
        room.items.retain(|i| !(i.item == item_id && i.pending));
        if room.items.len() != before {
            room.pending = false;
        }
    }
    match avatar_of(svc, ctx, owner) {
        Some(a) => {
            if let Err(e) = create_item_or_mail(
                svc,
                ctx,
                PlayerRef(a),
                spare.clone(),
                ItemOrigin::Other("housing_undo"),
                None,
                None,
            ) {
                tracing::error!(
                    owner = owner.0,
                    item = %crate::item::fx::item_json(&spare),
                    "housing: the item could not be created again: {e}"
                );
            }
        }
        None => tracing::error!(
            owner = owner.0,
            item = %crate::item::fx::item_json(&spare),
            "housing: the list-up failed and the owner is gone; the item is lost unless restored"
        ),
    }
}

// ---- HousingSetupFurnitureReq ------------------------------------------------------------------------------------

/// `HousingSetupFurnitureReq` (`CHousingMng::ReqSetupFurniture`, `CHousing::IsSetupAble`, `SetupFurniture`): install or
/// remove a listed piece in the own room. Every refusal is silent in this order: no `info`; no own room; a list-up in
/// flight; not in world 209 on the own layer; the item is not listed or its entry is pending; the state does not change;
/// no prop; installing: the same-kind count (KQ-18) answers `INSTALL_FAIL02`; installing: the position or angle is not
/// finite or the position lies outside the world (FIX, the original placed the ctrl anywhere).
pub fn on_setup_furniture(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::HousingSetupFurnitureReq,
) {
    let Some(info) = msg.info else {
        return;
    };
    let Some(owner) = character_of(ctx, who) else {
        return;
    };
    let Some(room) = svc.event.housing.room(owner) else {
        return;
    };
    if room.pending {
        return;
    }
    let in_own_room = ctx
        .player(who)
        .is_some_and(|m| m.core.world == ROOM_WORLD && Some(m.core.layer) == room_layer(owner));
    if !in_own_room {
        return;
    }
    let Some(index) = room.index_of(info.item_id) else {
        return;
    };
    let Some(entry) = room.items.get(index) else {
        return;
    };
    if entry.pending || entry.installed == info.setup {
        return;
    }
    let Some(prop) = ctx.data.item(info.item_id) else {
        return;
    };
    let mut place = None;
    if info.setup {
        // KQ-18: `IsSetupAble` counts the installed pieces of the same kind3; the `IK3_PROPS` case allowed two but fell
        // through into `default` (one): both checks are kept literally
        let count = room
            .items
            .iter()
            .filter(|i| i.installed)
            .filter(|i| {
                ctx.data
                    .item(i.item)
                    .is_some_and(|p| p.item_kind3 == prop.item_kind3)
            })
            .count();
        if (prop.item_kind3 == IK3_PROPS && count >= 2) || count >= 1 {
            notice::defined(ctx, who, TID_GAME_HOUSING_INSTALL_FAIL02, Vec::new());
            return;
        }
        let Some(pos) = info.pos else {
            return;
        };
        let pos = Vec3::new(pos.x, pos.y, pos.z);
        let inside = ctx
            .worlds
            .world(ROOM_WORLD)
            .is_some_and(|w| w.statics.in_world3(pos));
        if !(pos.x.is_finite() && pos.y.is_finite() && pos.z.is_finite() && info.angle.is_finite())
            || !inside
        {
            return;
        }
        place = Some((pos, info.angle));
    }
    // ---- apply ----
    let Some(room) = svc.event.housing.rooms_mut().get_mut(&owner) else {
        return;
    };
    let Some(entry) = room.items.get_mut(index) else {
        return;
    };
    let before = entry.clone();
    entry.installed = info.setup;
    if let Some((pos, angle)) = place {
        entry.pos = pos;
        entry.angle = angle;
    }
    let record = entry.record(owner);
    let wire = entry.info();
    let after = entry.clone();
    // the server's keep time and listing time ride in the record; the client's relative time was never read
    ctx.persist.op(DbOp::UpsertHousingFurniture(record));
    send_to(ctx, who, pb::HousingSetupFurnitureNtf { info: Some(wire) });
    if info.setup {
        let obj = put_up(svc, ctx, owner, Some(who.0), &after);
        if let Some(room) = svc.event.housing.rooms_mut().get_mut(&owner)
            && let Some(e) = room.items.get_mut(index)
        {
            e.obj = obj;
        }
    } else {
        take_down(svc, ctx, owner, Some(who.0), &before);
        if let Some(room) = svc.event.housing.rooms_mut().get_mut(&owner)
            && let Some(e) = room.items.get_mut(index)
        {
            e.obj = None;
        }
    }
}

// ---- visitors ----------------------------------------------------------------------------------------------------

/// `HousingSetVisitAllowReq` (`CHousingMng::ReqSetAllowVisit`, `CHousing::SetVisitAllow`): the target must be a
/// messenger friend (so never the owner) and the permission must change. The visitor list changes in memory and in the
/// table, the owner is told, and a visitor who is revoked while he stands in the room is sent out.
pub fn on_set_visit_allow(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::HousingSetVisitAllowReq,
) {
    let Some(owner) = character_of(ctx, who) else {
        return;
    };
    let target = CharacterId(msg.target);
    if !svc
        .social
        .messenger
        .get(owner)
        .is_some_and(|m| m.is_friend(target))
    {
        return;
    }
    let Some(room) = svc.event.housing.rooms_mut().get_mut(&owner) else {
        return;
    };
    if room.is_allowed(target) == msg.allow {
        return;
    }
    if msg.allow {
        room.visitors.insert(target);
    } else {
        room.visitors.remove(&target);
    }
    ctx.persist.op(DbOp::SetHousingVisitor {
        owner_id: owner,
        visitor_id: target,
        allowed: msg.allow,
    });
    send_to(
        ctx,
        who,
        pb::HousingSetVisitAllowNtf {
            target: target.0,
            allow: msg.allow,
        },
    );
    if !msg.allow {
        kick_if_inside(svc, ctx, owner, target);
    }
}

/// `GoOut` for `visitor` when he stands in the room of `owner`.
fn kick_if_inside(svc: &Services, ctx: &mut GameCtx<'_>, owner: CharacterId, visitor: CharacterId) {
    let Some(layer) = room_layer(owner) else {
        return;
    };
    let Some(avatar) = avatar_of(svc, ctx, visitor) else {
        return;
    };
    if ctx
        .worlds
        .core(avatar)
        .is_some_and(|c| c.world == ROOM_WORLD && c.layer == layer)
    {
        go_out(ctx, PlayerRef(avatar));
    }
}

/// `HousingReqVisitableList` (`OnReqVisitableList`): the loaded rooms the player may visit, sent only when there is one.
pub fn on_req_visitable_list(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let owners: Vec<u32> = svc
        .event
        .housing
        .visitable_for(me)
        .into_iter()
        .map(|c| c.0)
        .collect();
    if !owners.is_empty() {
        send_to(ctx, who, pb::HousingVisitableList { owners });
    }
}

// ---- visit and go out --------------------------------------------------------------------------------------------

/// `HousingVisitRoom` (`CHousingMng::SetVisitRoom`).
pub fn on_visit_room(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::HousingVisitRoom,
) {
    visit(svc, ctx, who, CharacterId(msg.owner));
}

/// Visits the room of `owner` (the API a server-side `/hv` would call): his room is loaded (he is in the world); a
/// player who is not a game master must be allowed (himself always) AND stand at a `MMI_VISIT_FRIEND` NPC (VR-012, also
/// for his own room: the NPC menu is the only way in); a game master bypasses both; the room layer must exist and be live.
/// The replace is a `Normal` one (a flying player who is not a game master is refused by it); on success the visitor
/// faces 175 and gets the caption `TID_GAME_HOUSING_ENTER "<owner name>"`.
pub fn visit(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, owner: CharacterId) {
    let Some(me) = character_of(ctx, who) else {
        return;
    };
    let Some(room) = svc.event.housing.room(owner) else {
        return;
    };
    let Some(m) = ctx.player(who) else {
        return;
    };
    let gm = m.player().is_some_and(|p| p.ident.authority.is_gm());
    if !gm {
        let near = ctx
            .worlds
            .close_npc(MMI_VISIT_FRIEND, m.core.world, m.core.layer, m.core.pos)
            .is_some();
        if !room.is_allowed(me) || !near {
            return;
        }
    }
    let Some(layer) = room_layer(owner).filter(|_| layer_live(ctx, owner)) else {
        return;
    };
    if ctx
        .worlds
        .replace(who.0, ROOM_WORLD, ROOM_ENTRY, layer, ReplaceKind::Normal)
        .is_err()
    {
        return;
    }
    let angle = flyff_sim::num::set_angle_quantized(ROOM_ENTRY_ANGLE);
    if let Some(m) = ctx.worlds.mover_mut(who.0) {
        m.sim.angle = angle;
        m.core.angle = angle;
    }
    let name = svc
        .presence
        .get(owner)
        .map(|e| e.name.clone())
        .unwrap_or_default();
    send_to(
        ctx,
        who,
        pb::DefinedText {
            tid: i32::try_from(TID_GAME_HOUSING_ENTER).unwrap_or(i32::MAX),
            args: vec![format!("\"{name}\"")],
            display: pb::defined_text::Display::Caption as i32,
        },
    );
}

/// `HousingGoOut` (`CHousingMng::GoOut`): only inside world 209 (VR-062 FIX: the original teleported from anywhere).
pub fn on_go_out(_svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if ctx.player(who).is_some_and(|m| m.core.world == ROOM_WORLD) {
        go_out(ctx, who);
    }
}

// ---- seams -------------------------------------------------------------------------------------------------------

/// The owner of a loaded room lost `visitor` from his list: he is told, and the visitor is sent out when inside.
fn revoked(svc: &mut Services, ctx: &mut GameCtx<'_>, owner: CharacterId, visitor: CharacterId) {
    let had = svc
        .event
        .housing
        .rooms_mut()
        .get_mut(&owner)
        .is_some_and(|r| r.visitors.remove(&visitor));
    if !had {
        return;
    }
    super::room::tell_owner(
        svc,
        ctx,
        owner,
        pb::HousingSetVisitAllowNtf {
            target: visitor.0,
            allow: false,
        },
    );
    kick_if_inside(svc, ctx, owner, visitor);
}

/// The messenger seam (`friendship_ended`, called after the two lists were unlinked): the visit permission goes both
/// ways (`CDPCoreClient::OnRemoveFriend`). A loaded room drops the visitor, tells its owner and sends the visitor out; the
/// delete of the row is written for both directions even when an owner is offline (FIX: the original revoked only
/// online owners and relied on the next load).
pub fn friendship_ended(svc: &mut Services, ctx: &mut GameCtx<'_>, a: CharacterId, b: CharacterId) {
    for (owner, visitor) in [(a, b), (b, a)] {
        revoked(svc, ctx, owner, visitor);
        ctx.persist.op(DbOp::SetHousingVisitor {
            owner_id: owner,
            visitor_id: visitor,
            allowed: false,
        });
    }
}

/// The deletion seam (`SocialDeleteHooks::apply`, 21 §3.6): every loaded room drops `c` and the delete of all his
/// visits joins the deletion batch (the game knows the rooms of the characters in the world only).
pub fn on_character_deleted(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    ops: &mut Vec<DbOp>,
) {
    let owners: Vec<CharacterId> = svc
        .event
        .housing
        .rooms()
        .filter(|r| r.visitors.contains(&c))
        .map(|r| r.owner)
        .collect();
    for owner in owners {
        revoked(svc, ctx, owner, c);
    }
    ops.push(DbOp::DeleteHousingVisitsOf { character_id: c });
}
