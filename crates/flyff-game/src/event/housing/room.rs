//! The room itself: the load at the join, the layer and its furniture ctrls, the buffs, the papering, the logout and
//! the drive-out (`CHousing::AddFurnitureControl`, `RemoveFurnitureControl`, `CHousingMng::CreateRoomLayer`,
//! `DestroyHousing`, `GoOut`; Plan/22 §13.1, ref/15 §14).
//!
//! **Load normalisation** (13 §17.4 step 7; this replaces a `LoadHook`, which has no `Services`): the rows of
//! `CharacterLoad.housing` become the list. A row whose prop is missing or is not an `IK1_HOUSING` item is skipped with
//! a warning and its row is kept (the data is wrong, not the player). A row with `keep_until <= now` is deleted.
//! A visitor who is no longer a messenger friend is deleted from the visitor list.

use super::{
    GO_OUT, GO_OUT_ANGLE, Housing, HousingItem, PAPERING_RESET, ROOM_WORLD, owner_of_layer,
    room_layer,
};
use crate::chat::privacy::by_character;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::{Ctrl, LayerError, LayerKind, LayerState, ReplaceKind, Worlds};
use flyff_common::defs::itemkind::{IK1_HOUSING, IK2_PAPERING};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use flyff_data::worlds::CtrlElemDef;
use flyff_db::{CharacterLoad, DbOp};
use flyff_proto::pb;

/// The character of a live player.
pub(super) fn character_of(ctx: &GameCtx<'_>, p: PlayerRef) -> Option<CharacterId> {
    ctx.player(p)?.player().map(|pl| pl.ident.character_id)
}

/// The avatar of an online character (`None` when he is offline).
pub(super) fn avatar_of(svc: &Services, ctx: &GameCtx<'_>, c: CharacterId) -> Option<ObjId> {
    by_character(svc, ctx.worlds, c)
}

/// The players inside the room layer (linked or entering).
pub(super) fn players_in_room(ctx: &GameCtx<'_>, layer: LayerId) -> Vec<ObjId> {
    let Some(l) = ctx.worlds.layer(ROOM_WORLD, layer) else {
        return Vec::new();
    };
    l.members
        .iter()
        .copied()
        .chain(l.queues.add.iter().copied())
        .filter(|&o| ctx.worlds.store().is_player(o))
        .collect()
}

/// Queues `msg` for the avatar `to`.
pub(super) fn tell(ctx: &mut GameCtx<'_>, to: ObjId, msg: impl Into<pb::server_msg::Msg>) {
    send_to(ctx, PlayerRef(to), msg);
}

/// Queues `msg` for `owner` when he is in the world.
pub(super) fn tell_owner(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    msg: impl Into<pb::server_msg::Msg>,
) {
    if let Some(o) = avatar_of(svc, ctx, owner) {
        tell(ctx, o, msg);
    }
}

/// `HousingPaperingInfo{item, setup}` to `to`.
fn send_papering(ctx: &mut GameCtx<'_>, to: ObjId, item: u32, setup: bool) {
    tell(
        ctx,
        to,
        pb::HousingPaperingInfo {
            item_id: item,
            setup,
        },
    );
}

/// The papering set of `owner`'s room for one player (`World.cpp:1656-1670`): the reset, then one message per installed
/// paper in list order (the reset alone for a room without paper).
pub(super) fn send_papering_set(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    to: ObjId,
    owner: CharacterId,
) {
    send_papering(ctx, to, PAPERING_RESET, false);
    for item in svc.event.housing.papering_of(owner) {
        send_papering(ctx, to, item, true);
    }
}

/// A paper installed or taken down: every player in the room sees it.
fn papering_to_room(ctx: &mut GameCtx<'_>, owner: CharacterId, item: u32, setup: bool) {
    let Some(layer) = room_layer(owner) else {
        return;
    };
    for o in players_in_room(ctx, layer) {
        send_papering(ctx, o, item, setup);
    }
}

/// The ctrl of a piece of furniture at `pos` on the owner's layer (`CreateObj(OT_CTRL, dwLinkKind)`,
/// `ADDOBJ(pCtrl, FALSE, layer)`). A missing ctrl prop or a refused spawn is logged and gives `None`: the item stays
/// installed.
fn spawn_piece(
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    item: u32,
    pos: Vec3,
    angle: f32,
) -> Option<ObjId> {
    let layer = room_layer(owner)?;
    let link = ctx.data.item(item)?.link_kind;
    if ctx.data.ctrls.get(link).is_none() {
        tracing::warn!(
            owner = owner.0,
            item,
            link,
            "housing: the furniture has no ctrl prop"
        );
        return None;
    }
    let ctrl = Ctrl::new(link, pos, angle, CtrlElemDef::default(), false);
    match ctx.worlds.spawn_ctrl(ROOM_WORLD, layer, ctrl) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(
                owner = owner.0,
                item,
                "housing: the furniture ctrl was refused: {e}"
            );
            None
        }
    }
}

/// `SetBuff(item, TRUE)`: the item's buff on `avatar`. A refusal is logged.
pub(super) fn apply_buff(svc: &mut Services, ctx: &mut GameCtx<'_>, avatar: ObjId, item: u32) {
    if let Err(e) = crate::buff::api::apply_item(svc, ctx, avatar, item, None) {
        tracing::debug!(
            avatar = avatar.0,
            item,
            "housing: the furniture buff was refused: {e:?}"
        );
    }
}

/// An entry was installed: a paper is shown to the room, a piece of furniture gets its ctrl (the returned object id),
/// and the item's buff is applied to the owner. FIX (open question 8): the buff follows the installed state even
/// when the ctrl cannot be spawned.
pub(super) fn put_up(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    avatar: Option<ObjId>,
    entry: &HousingItem,
) -> Option<ObjId> {
    let obj = if entry.paper {
        papering_to_room(ctx, owner, entry.item, true);
        None
    } else {
        spawn_piece(ctx, owner, entry.item, entry.pos, entry.angle)
    };
    if let Some(a) = avatar {
        apply_buff(svc, ctx, a, entry.item);
    }
    obj
}

/// An entry was taken down (removed, expired): the inverse of [`put_up`]. `avatar` is the owner's avatar when he is
/// in the world.
pub(super) fn take_down(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    owner: CharacterId,
    avatar: Option<ObjId>,
    entry: &HousingItem,
) {
    if entry.paper {
        papering_to_room(ctx, owner, entry.item, false);
    } else if let Some(o) = entry.obj {
        ctx.worlds.delete(o);
    }
    if let Some(a) = avatar {
        crate::buff::api::remove_item(svc, ctx, a, entry.item);
    }
}

/// The room of a joining character (`OnHousingLoadInfo`, `CreateRoomLayer`): the list from the load, the layer, the
/// ctrls of the installed furniture. Runs right after the placement and the messenger load of the join, before the
/// join batch (so `HousingAllInfo` finds the room).
pub fn on_join(svc: &mut Services, ctx: &mut GameCtx<'_>, _p: PlayerRef, load: &CharacterLoad) {
    let owner = load.identity.id;
    let now = ctx.clock.unix_ms.to_unix_time();
    // a room that a session left behind without `DestroyHousing` (a tainted leave): its ctrls go first
    if let Some(old) = svc.event.housing.rooms_mut().remove(&owner) {
        for o in old.items.iter().filter_map(|i| i.obj) {
            ctx.worlds.delete(o);
        }
    }
    let mut room = Housing::new(owner);
    for row in &load.housing.items {
        let Some(prop) = ctx
            .data
            .item(row.item_id)
            .filter(|p| p.item_kind1 == IK1_HOUSING)
        else {
            tracing::warn!(
                owner = owner.0,
                item = row.item_id,
                "housing: the stored piece is not a housing item; skipped, the row stays"
            );
            continue;
        };
        if row.keep_until <= now {
            ctx.persist.op(DbOp::DeleteHousingFurniture {
                character_id: owner,
                item_id: row.item_id,
            });
            continue;
        }
        room.items.push(HousingItem {
            item: row.item_id,
            keep_until: row.keep_until,
            installed: row.installed,
            pos: Vec3::new(row.pos[0], row.pos[1], row.pos[2]),
            angle: row.angle,
            listed_at: row.listed_at,
            obj: None,
            pending: false,
            paper: prop.item_kind2 == IK2_PAPERING,
        });
    }
    // `CHousing::Serialize`: a loaded visitor who is no longer a friend is deleted; without a friend list nobody can tell
    let friends = svc.social.messenger.get(owner);
    for &v in &load.housing.visitors {
        if friends.is_none_or(|m| m.is_friend(v)) {
            room.visitors.insert(v);
        } else {
            ctx.persist.op(DbOp::SetHousingVisitor {
                owner_id: owner,
                visitor_id: v,
                allowed: false,
            });
        }
    }
    create_layer(svc, ctx, owner);
    for entry in room.items.iter_mut().filter(|i| i.installed && !i.paper) {
        entry.obj = spawn_piece(ctx, owner, entry.item, entry.pos, entry.angle);
    }
    svc.event.housing.rooms_mut().insert(owner, room);
}

/// `CreateRoomLayer`: the layer of the owner's room. `Exists` is an invalidated layer of the previous session that
/// still drains its visitors: it is revalidated (FIX: the original released it under the online owner and the room
/// stayed unenterable until the next login).
fn create_layer(svc: &Services, ctx: &mut GameCtx<'_>, owner: CharacterId) {
    let Some(layer) = room_layer(owner) else {
        tracing::error!(
            owner = owner.0,
            "housing: the character id has no room layer"
        );
        return;
    };
    match ctx.worlds.create_layer(
        ROOM_WORLD,
        layer,
        LayerKind::MiniRoom,
        ctx.data,
        &svc.event.flags,
        ctx.rng.event(),
    ) {
        Ok(()) => {}
        Err(LayerError::Exists) => {
            ctx.worlds.revalidate(ROOM_WORLD, layer);
        }
        Err(LayerError::WorldMissing) => {
            tracing::debug!(owner = owner.0, "housing: world 209 is not loaded; no room");
        }
        Err(e) => tracing::warn!(owner = owner.0, "housing: the room layer was refused: {e}"),
    }
}

/// After the join batch (`SessionRules::joined`, so the buff messages follow `SpawnSelf`): the buffs of the installed
/// items of the own room (papers too: `AddFurnitureControl` called `SetBuff` for both kinds), and, when the join
/// placed the player in world 209 (a crash-time location inside a room whose owner is online), the papering set of
/// that room, or the way out when he is no longer allowed there (FIX).
pub fn after_join(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some(me) = character_of(ctx, p) else {
        return;
    };
    let installed: Vec<u32> = svc
        .event
        .housing
        .room(me)
        .map(|r| {
            r.items
                .iter()
                .filter(|i| i.installed)
                .map(|i| i.item)
                .collect()
        })
        .unwrap_or_default();
    for item in installed {
        apply_buff(svc, ctx, p.0, item);
    }
    enter_check(svc, ctx, p, me);
}

/// A re-attached session reloads the world from the replayed batch: a player inside a room needs the papering set again.
/// The buffs are in memory and need no work.
pub fn after_reattach(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    if let Some(me) = character_of(ctx, p) {
        enter_check(svc, ctx, p, me);
    }
}

/// A player who stands in world 209: the papering set of his room, or out when he may not be there.
fn enter_check(svc: &Services, ctx: &mut GameCtx<'_>, p: PlayerRef, me: CharacterId) {
    let Some(layer) = ctx
        .player(p)
        .filter(|m| m.core.world == ROOM_WORLD)
        .map(|m| m.core.layer)
    else {
        return;
    };
    // only a loaded room is acted on: layer 0 of world 209 (a GM teleport) and the layer of an owner who is gone are
    // not his business (the garbage collection drives the latter out)
    let Some(owner) = owner_of_layer(layer).filter(|&o| svc.event.housing.room(o).is_some()) else {
        return;
    };
    if svc.event.housing.is_allowed(owner, me) {
        send_papering_set(svc, ctx, p.0, owner);
    } else {
        drive_out(ctx.worlds, p.0);
    }
}

/// `RoomEntered` (the note of `EventWorldHooks::world_change_entered`, drained after the world phase): `player` came into
/// the room layer `layer` by a replace. If the room is loaded and he is still there, he gets the papering set
/// (`World.cpp:1656-1670`: the reset first, then one message per installed paper; the reset alone for a room without
/// paper).
pub fn on_room_entered(svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId, layer: LayerId) {
    let still = ctx
        .worlds
        .player(player)
        .is_some_and(|m| m.core.world == ROOM_WORLD && m.core.layer == layer);
    let Some(owner) =
        owner_of_layer(layer).filter(|&o| still && svc.event.housing.room(o).is_some())
    else {
        return;
    };
    send_papering_set(svc, ctx, player, owner);
}

/// `LogoutStep::DestroyHousing` (`CHousingMng::DestroyHousing`, run at every logout): the ctrls of the leaver's room are
/// deleted, its layer is invalidated (the garbage collection drives the visitors out through
/// [`drive_out`] and releases the layer) and the room is dropped. The saved location of a player who stands in a room is
/// already Flaris by `destroy_player`.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some(owner) = character_of(ctx, p) else {
        return;
    };
    let Some(room) = svc.event.housing.rooms_mut().remove(&owner) else {
        return;
    };
    for o in room.items.iter().filter_map(|i| i.obj) {
        ctx.worlds.delete(o);
    }
    if let Some(layer) = room_layer(owner) {
        ctx.worlds.invalidate(ROOM_WORLD, layer);
    }
}

/// `CHousingMng::GoOut`: Madrigal `(6985, 0, 3234)`, layer 0, forced, facing 50. The angle is set once the replace was
/// accepted (the original set it unconditionally; a refused replace left the player in place facing 50). The
/// `EventDriveOut` arm of `LayerKind::MiniRoom` and every go-out use it.
pub fn drive_out(worlds: &mut Worlds, player: ObjId) {
    match worlds.replace(
        player,
        WorldId(WI_WORLD_MADRIGAL),
        GO_OUT,
        LayerId(0),
        ReplaceKind::Force,
    ) {
        Ok(()) => {
            let angle = flyff_sim::num::set_angle_quantized(GO_OUT_ANGLE);
            if let Some(m) = worlds.mover_mut(player) {
                m.sim.angle = angle;
                m.core.angle = angle;
            }
        }
        Err(e) => tracing::debug!(player = player.0, "housing: the go-out was refused: {e}"),
    }
}

/// [`drive_out`] for a player reference (the handlers' form).
pub fn go_out(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    drive_out(ctx.worlds, who.0);
}

/// `true` while the layer of `owner`'s room exists and is live (not waiting for its garbage collection).
pub(super) fn layer_live(ctx: &GameCtx<'_>, owner: CharacterId) -> bool {
    room_layer(owner)
        .and_then(|l| ctx.worlds.layer(ROOM_WORLD, l))
        .is_some_and(|l| l.state == LayerState::Live)
}
