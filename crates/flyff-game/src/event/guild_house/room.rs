//! The room of a guild house: the layer `(212, LayerId(guild id))` and its objects (Plan/22 §12.2;
//! `_Common/GuildHouse.cpp:306-406`).
//!
//! * [`ensure_room`] (`CreateGuildHouseRoom`): a live room is left alone; a room waiting for its garbage collection is
//!   revalidated and keeps its live objects (FIX: the original cleared the ids at destroy and did not respawn on a
//!   re-entry before the GC, so the furniture could not be selected); an absent room is created and every installed
//!   entry spawned (a furniture entry is a ctrl, a teleporter an NPC mover, papering no object); each installed entry
//!   is announced with a `REFRESH`.
//! * [`check_destroy_room`] (`CheckDestroyRoom`): nobody but `except` in or entering the room: `REFRESH` with no
//!   object ids, then `invalidate` (the garbage collection releases it; the drive-out of an occupied invalidated room
//!   is `MadrigalDriveOut`'s `revalidate`, KEEP `WorldFile.cpp:1300-1306`).
//! * [`go_out`] (`GoOutGuildHouseRoom`): the marking position in Madrigal, forced.
//! * [`login_admission`] (`IsLoginAble`), [`on_logout`] (`LogoutStep::GuildHouseGoOut`) and [`on_layer_changed`]
//!   (the `LayerChanged` note of the world phase).

use super::{
    COMEBACK_POS, GhFurniture, GhOp, WORLD, fntr_info_at, guild_of_layer, is_guild_house_world,
    layer_of, notify,
};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::world::join::SavedLocation;
use crate::world::{Ctrl, LayerKind, LayerState, ReplaceKind, Worlds};
use flyff_common::defs::itemkind::{IK2_GUILDHOUSE_FURNITURE, IK2_GUILDHOUSE_NPC};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::{CharacterId, GuildId, LayerId, ObjId, WorldId};
use flyff_data::worlds::CtrlElemDef;
use flyff_proto::pb;

/// `true` when the object still exists in the room.
fn object_alive(worlds: &Worlds, f: &GhFurniture) -> bool {
    f.obj
        .and_then(|o| worlds.core(o))
        .is_some_and(|c| !c.deleted)
}

/// The players in or entering the room (linked, waiting in the add queue or targeted by a queued replace), `except`
/// left out.
pub(crate) fn occupied(worlds: &Worlds, layer: LayerId, except: Option<ObjId>) -> bool {
    let Some(l) = worlds.layer(WORLD, layer) else {
        return false;
    };
    let inside = l
        .members
        .iter()
        .chain(l.queues.add.iter())
        .any(|&o| Some(o) != except && worlds.store().is_player(o));
    inside
        || worlds.worlds_iter().any(|w| {
            w.layers.values().any(|other| {
                other
                    .queues
                    .replace
                    .iter()
                    .any(|e| e.world == WORLD && e.layer == layer && Some(e.id) != except)
            })
        })
}

/// The players in the room, linked or waiting to be added.
pub(crate) fn players_inside(worlds: &Worlds, layer: LayerId) -> Vec<ObjId> {
    let Some(l) = worlds.layer(WORLD, layer) else {
        return Vec::new();
    };
    let mut v: Vec<ObjId> = l
        .members
        .iter()
        .chain(l.queues.add.iter())
        .copied()
        .filter(|&o| worlds.store().is_player(o))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Spawns the object of an installed entry in the room (`SetupFurnitureCtrl`): kind 38 a ctrl of the prop's
/// `link_kind`, kind 39 an NPC mover of `link_kind` with the character key `text_file`; papering and everything else
/// has no object. A missing prop or a refused spawn is logged and the entry stays installed.
pub(crate) fn spawn_object(
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    f: &GhFurniture,
) -> Option<ObjId> {
    let layer = layer_of(guild)?;
    let data = ctx.data;
    let Some(prop) = data.item(f.item) else {
        tracing::warn!(
            guild = guild.0,
            item = f.item,
            "guild house: a furniture item has no prop"
        );
        return None;
    };
    match prop.item_kind2 {
        IK2_GUILDHOUSE_FURNITURE => {
            let ctrl = Ctrl::new(
                prop.link_kind,
                f.pos,
                f.angle,
                CtrlElemDef::default(),
                false,
            );
            match ctx.worlds.spawn_ctrl(WORLD, layer, ctrl) {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::warn!(guild = guild.0, item = f.item, error = %e, "guild house: a furniture ctrl was refused");
                    None
                }
            }
        }
        IK2_GUILDHOUSE_NPC => {
            let Some(mover_prop) = data.mover(prop.link_kind) else {
                tracing::warn!(
                    guild = guild.0,
                    item = f.item,
                    mover = prop.link_kind,
                    "guild house: an NPC furniture has no mover prop"
                );
                return None;
            };
            let (core, sim) = crate::mover::npc_parts(data, mover_prop, f.pos, f.angle);
            let mover = Mover::new_npc(
                core,
                sim,
                mover_prop,
                data.npc(&prop.text_file),
                data,
                ctx.rng.world(WORLD),
            );
            match ctx.worlds.spawn_mover(WORLD, layer, mover) {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::warn!(guild = guild.0, item = f.item, error = %e, "guild house: an NPC furniture was refused");
                    None
                }
            }
        }
        _ => None,
    }
}

/// Spawns the objects of the installed entries (`only_missing`: only those whose object is gone) and announces each
/// installed entry with a `REFRESH{index, info}` to the members.
fn spawn_and_refresh(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    only_missing: bool,
) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let jobs: Vec<(usize, GhFurniture)> = house
        .installed()
        .filter(|(_, f)| !only_missing || !object_alive(ctx.worlds, f))
        .map(|(i, f)| (i, f.clone()))
        .collect();
    let spawned: Vec<(usize, Option<ObjId>)> = jobs
        .into_iter()
        .map(|(i, f)| (i, spawn_object(ctx, guild, &f)))
        .collect();
    if let Some(house) = svc.event.guild_houses.house_mut(guild) {
        for (i, obj) in spawned {
            if let Some(f) = house.furniture.get_mut(i) {
                f.obj = obj;
            }
        }
    }
    refresh_all(svc, ctx, guild);
}

/// `GuildHousePacketNtf{REFRESH, i, info}` for every installed entry (the object ids follow the live room).
pub(crate) fn refresh_all(svc: &Services, ctx: &mut GameCtx<'_>, guild: GuildId) {
    let Some(house) = svc.event.guild_houses.house(guild) else {
        return;
    };
    let live = super::room_live(ctx.worlds, guild);
    let msgs: Vec<(i32, pb::GhFntrInfo)> = house
        .installed()
        .map(|(i, f)| (i32::try_from(i).unwrap_or(i32::MAX), fntr_info_at(f, live)))
        .collect();
    for (i, info) in msgs {
        notify(svc, ctx, guild, GhOp::Refresh, i, info);
    }
}

/// `CreateGuildHouseRoom` (see the module doc). `true` when the room exists and is live afterwards.
pub(crate) fn ensure_room(svc: &mut Services, ctx: &mut GameCtx<'_>, guild: GuildId) -> bool {
    let Some(layer) = layer_of(guild) else {
        tracing::warn!(
            guild = guild.0,
            "guild house: the guild id does not fit a layer id: no room"
        );
        return false;
    };
    if !svc.event.guild_houses.has_house(guild) {
        return false;
    }
    match ctx.worlds.layer(WORLD, layer).map(|l| l.state) {
        Some(LayerState::Live) => true,
        Some(LayerState::Invalidated) => {
            ctx.worlds.revalidate(WORLD, layer);
            spawn_and_refresh(svc, ctx, guild, true);
            true
        }
        None => {
            let created = ctx.worlds.create_layer(
                WORLD,
                layer,
                LayerKind::GuildHouse,
                ctx.data,
                &svc.event.flags,
                ctx.rng.event(),
            );
            if let Err(e) = created {
                tracing::warn!(guild = guild.0, error = %e, "guild house: the room could not be created");
                return false;
            }
            spawn_and_refresh(svc, ctx, guild, false);
            true
        }
    }
}

/// `CheckDestroyRoom(except)`: see the module doc.
pub(crate) fn check_destroy_room(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    except: Option<ObjId>,
) {
    let Some(layer) = layer_of(guild) else {
        return;
    };
    if ctx.worlds.layer(WORLD, layer).map(|l| l.state) != Some(LayerState::Live) {
        return;
    }
    if occupied(ctx.worlds, layer, except) {
        return;
    }
    if let Some(house) = svc.event.guild_houses.house(guild) {
        let msgs: Vec<(i32, pb::GhFntrInfo)> = house
            .installed()
            .map(|(i, f)| (i32::try_from(i).unwrap_or(i32::MAX), fntr_info_at(f, false)))
            .collect();
        for (i, info) in msgs {
            notify(svc, ctx, guild, GhOp::Refresh, i, info);
        }
    }
    ctx.worlds.invalidate(WORLD, layer);
}

/// The player stands in the room of `guild`.
pub(crate) fn is_inside(ctx: &GameCtx<'_>, p: ObjId, guild: GuildId) -> bool {
    ctx.worlds
        .core(p)
        .is_some_and(|c| c.world == WORLD && Some(c.layer) == layer_of(guild))
}

/// `GoOutGuildHouseRoom`: a player in the room of `guild` is sent to Madrigal (`REPLACE_FORCE`, layer 0) at his
/// marking position; a mark in another world or outside Madrigal (a foreign Lodelight mark after a login inside) takes
/// the comeback point instead (FIX). `true` when the replace was queued.
pub(crate) fn go_out(ctx: &mut GameCtx<'_>, guild: GuildId, p: ObjId) -> bool {
    if !is_inside(ctx, p, guild) {
        return false;
    }
    let madrigal = WorldId(WI_WORLD_MADRIGAL);
    let Some((mark_world, mark_pos)) = ctx
        .worlds
        .player(p)
        .and_then(|m| m.player())
        .map(|pl| (pl.misc.marking_world, pl.misc.marking_pos))
    else {
        return false;
    };
    let usable = mark_world == madrigal
        && ctx
            .worlds
            .world(madrigal)
            .is_some_and(|w| w.statics.in_world3(mark_pos));
    let pos = if usable { mark_pos } else { COMEBACK_POS };
    match ctx
        .worlds
        .replace(p, madrigal, pos, LayerId(0), ReplaceKind::Force)
    {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(player = p.0, error = %e, "guild house: the player could not be sent out");
            false
        }
    }
}

// ---- login, logout, layer change --------------------------------------------------------------------------------------

/// `IsLoginAble` (22 §12.2): a character saved in a guild-house world is admitted only into world 212, on the layer of
/// his own guild, with the house enterable (`upkeep_until > now`); the room is created so that the placement finds the
/// layer. Every other world admits (`true`) and creates nothing. A refusal sends no text (FIX: the original's
/// `IsEnteranceAble` sent `EXPIRATION` before the join batch); the placement then falls back to Madrigal.
pub fn login_admission(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    saved: SavedLocation,
) -> bool {
    if !is_guild_house_world(saved.world) {
        return true;
    }
    if saved.world != WORLD {
        return false;
    }
    let Some(guild) = svc.social.guild.guild_of(c) else {
        return false;
    };
    if guild_of_layer(saved.layer) != Some(guild) {
        return false;
    }
    let now = crate::social::party::now_secs(ctx);
    let enterable = svc
        .event
        .guild_houses
        .house(guild)
        .is_some_and(|h| h.upkeep_until.0 > now.0);
    enterable && ensure_room(svc, ctx, guild)
}

/// `LogoutStep::GuildHouseGoOut` (`User.cpp:4252-4254`, after the snapshot that saved the position inside): a player
/// leaving from his own house lets the room go when nobody else is in it.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some(m) = ctx.player(p) else {
        return;
    };
    if m.core.world != WORLD {
        return;
    }
    let own = m.player().and_then(|pl| pl.social.guild);
    let Some(guild) = guild_of_layer(m.core.layer).filter(|g| Some(*g) == own) else {
        return;
    };
    if svc.event.guild_houses.has_house(guild) {
        check_destroy_room(svc, ctx, guild, Some(p.0));
    }
}

/// The `LayerChanged` note of a player who left a layer (`World.cpp:1612`, `CheckDestroyGuildHouse` on a replace out):
/// leaving a guild-house room for another world or layer lets the room go when it is empty now.
pub fn on_layer_changed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _player: ObjId,
    from: (WorldId, LayerId),
    to: (WorldId, LayerId),
) {
    if !is_guild_house_world(from.0) || from == to {
        return;
    }
    let Some(guild) = guild_of_layer(from.1) else {
        return;
    };
    if svc.event.guild_houses.has_house(guild) {
        check_destroy_room(svc, ctx, guild, None);
    }
}
