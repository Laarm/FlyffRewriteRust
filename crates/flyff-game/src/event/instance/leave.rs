//! Leaving a dungeon: the counter and the destroy (`LeaveToOutside`, `DecreasePlayerCount`,
//! `DestroyDungeonLayer`), the logout, the login and drive-out `GoOut`, the 3-minute kick of a party leaver
//! (`CInstanceDungeonParty::SetPartyLeaveTime` / `Process`) and the 1 s pass (`InstanceDungeonBase.cpp` 142-162,
//! 539-588, 713-747, 985-1027; `InstanceDungeonParty.cpp` 38-95; `Mover.cpp` 2343-2350; `party.cpp` 143-160).

use super::{LEAVE_KICK_MS, layer_of, party_of_layer};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::revival::MADRIGAL_DRIVE_OUT;
use crate::world::{ReplaceKind, Worlds};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::{CharacterId, GameMs, LayerId, ObjId, PartyId, WorldId};
use flyff_db::audit::ev::INSTANCE_DESTROY;

/// `Mover.cpp:2343-2350` (`replace_leaving`, via the drain of the world notes): a player left `from` for `to`. Only
/// a change from a dungeon world to a world that is none counts as a leave (a move between two dungeons keeps the
/// counter, KEEP).
pub fn on_replace(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: ObjId,
    from: (WorldId, LayerId),
    to: (WorldId, LayerId),
) {
    let set = &svc.event.instances.worlds;
    if !set.contains(from.0) || set.contains(to.0) {
        return;
    }
    leave(svc, ctx, p, from.0, from.1);
}

/// `LogoutStep::InstanceGoOut`: the logout of a player who stands in a dungeon world runs the bookkeeping of a leave
/// (the original's `GoOut` `REPLACE` did). The saved location is `destroy_player`'s own (the Madrigal mark or the
/// Flaris centre).
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some((world, layer)) = ctx
        .worlds
        .player(who.0)
        .map(|m| (m.core.world, m.core.layer))
    else {
        return;
    };
    if !svc.event.instances.worlds.contains(world) {
        return;
    }
    leave(svc, ctx, who.0, world, layer);
}

/// `LeaveToOutside` for the layer `layer` of the dungeon world `world`: the counter goes down; at zero or less the
/// dungeon is destroyed unless an online member of the party other than `p` still stands in it.
fn leave(svc: &mut Services, ctx: &mut GameCtx<'_>, p: ObjId, world: WorldId, layer: LayerId) {
    let Some(party) = party_of_layer(layer) else {
        return;
    };
    let Some(inst) = svc.event.instances.instances.get_mut(&(world, party)) else {
        // already destroyed (or the layer 0 population of a GM)
        return;
    };
    inst.player_count -= 1;
    if inst.player_count > 0 {
        return;
    }
    let member_inside = svc.social.party.get(party).is_some_and(|pt| {
        pt.online()
            .filter_map(|s| s.obj)
            .filter(|&o| o != p)
            .any(|o| {
                ctx.worlds
                    .player(o)
                    .is_some_and(|m| m.core.world == world && m.core.layer == layer)
            })
    });
    if member_inside {
        return;
    }
    destroy(svc, ctx, p, world, party, layer);
}

/// `DestroyDungeonLayer` + `ResetDungeonCoolTimeInfo`: the layer is invalidated (the garbage collection releases it
/// once nobody is in it; the occupants are driven out every tick until then), the cooldowns of that dungeon now bind
/// its party too, and `instance.destroy` is audited with `actor` as the actor.
fn destroy(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: ObjId,
    world: WorldId,
    party: PartyId,
    layer: LayerId,
) {
    let Some(inst) = svc.event.instances.instances.remove(&(world, party)) else {
        return;
    };
    ctx.worlds.invalidate(world, layer);
    for list in svc.event.instances.cooldowns.values_mut() {
        for c in list
            .iter_mut()
            .filter(|c| c.world == world && c.party == Some(party))
        {
            c.party = None;
        }
    }
    crate::audit::emit_for(
        ctx,
        PlayerRef(actor),
        INSTANCE_DESTROY,
        serde_json::json!({
            "world": world.0,
            "layer": layer.0,
            "party": party.0,
            "state": inst.state.kind(),
            "players": inst.player_count,
        }),
    );
}

/// `CInstanceDungeonHelper::GoOut`: `REPLACE(marking world, marking pos, REPLACE_FORCE, layer 0)`. FIX: a marking
/// world 0 or not loaded, or a refused replace, falls back to Madrigal's drive-out point (the original left the
/// player in the invalidated layer).
pub fn go_out_worlds(worlds: &mut Worlds, p: ObjId) {
    let Some((world, pos)) = worlds
        .player(p)
        .and_then(|m| m.player())
        .map(|pl| (pl.misc.marking_world, pl.misc.marking_pos))
    else {
        return;
    };
    if world.0 != 0 && worlds.world(world).is_some() {
        match worlds.replace(p, world, pos, LayerId(0), ReplaceKind::Force) {
            Ok(()) => return,
            Err(e) => tracing::warn!(
                player = p.0,
                world = world.0,
                "instance: the marking position was refused: {e}"
            ),
        }
    }
    if let Err(e) = worlds.replace(
        p,
        WorldId(WI_WORLD_MADRIGAL),
        MADRIGAL_DRIVE_OUT,
        LayerId(0),
        ReplaceKind::Force,
    ) {
        tracing::debug!(
            player = p.0,
            "instance: the Madrigal fallback was refused: {e}"
        );
    }
}

/// `instance_go_out(player)` (the login of a character saved inside a dungeon, the leaver kick): the player is sent
/// to his marking position.
pub fn go_out(_svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    go_out_worlds(ctx.worlds, who.0);
}

/// `CParty::DeleteMember` -> `SetPartyLeaveTime(c)`: the removed member of a party who is online and stands in a
/// dungeon world gets three minutes (a second removal re-arms the timer). Only the removed member (KEEP: the remaining
/// member of a dissolved party gets no timer).
pub fn on_party_left(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    removed: CharacterId,
    _party: PartyId,
) {
    let Some(who) = crate::social::relation::avatar_of(svc, ctx, removed) else {
        return;
    };
    let Some(world) = ctx.worlds.player(who.0).map(|m| m.core.world) else {
        return;
    };
    if !svc.event.instances.worlds.contains(world) {
        return;
    }
    let deadline = GameMs(ctx.clock.game_ms.0 + LEAVE_KICK_MS);
    svc.event.instances.leave_timers.insert(removed, deadline);
}

/// 1 s slot item 8 `event::instance::process` (22 §2.3, §8.5): the due kick timers, then the cooldown pruning.
///
/// A character whose deadline passed and who is still online in a dungeon world with a layer other than his current
/// party's (or without a party) is sent out. FIX: an offline character is skipped, the loop goes on (the original
/// `return`ed).
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.game_ms;
    let due: Vec<CharacterId> = svc
        .event
        .instances
        .leave_timers
        .iter()
        .filter(|&(_, &t)| t < now)
        .map(|(&c, _)| c)
        .collect();
    for c in due {
        svc.event.instances.leave_timers.remove(&c);
        let Some(who) = crate::social::relation::avatar_of(svc, ctx, c) else {
            continue;
        };
        let Some((world, layer)) = ctx
            .worlds
            .player(who.0)
            .map(|m| (m.core.world, m.core.layer))
        else {
            continue;
        };
        if !svc.event.instances.worlds.contains(world) {
            continue;
        }
        let party_layer = svc.social.party.party_of(c).and_then(layer_of);
        if party_layer != Some(layer) {
            go_out(svc, ctx, who);
        }
    }
    // `UpdateDungeonCoolTimeInfo`: a character whose every record ended is forgotten
    svc.event
        .instances
        .cooldowns
        .retain(|_, list| list.iter().any(|c| c.until > now));
}
