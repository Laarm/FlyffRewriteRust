//! The progress of a dungeon: `SetInstanceDungeonKill`, `IsNowStateMonster`, `SetNextState`
//! (`InstanceDungeonBase.cpp` 444-493, 749-805; `Mover.cpp:5543-5548`).

use super::entry::spawn_state;
use super::{DungeonState, count_of, def_of, monsters_of, party_of_layer, teleport_of};
use crate::combat::KillCtx;
use crate::ctx::{GameCtx, Services};
use crate::world::ReplaceKind;
use flyff_common::{LayerId, ObjId, PartyId, WorldId};
use flyff_data::lua_config::DungeonDef;
use std::sync::Arc;

/// `DoDie` step 9 (`event::on_world_die`): a player killed a monster inside a dungeon world. The monster counts
/// when it is one of the current state's monsters; the last one moves the dungeon on. A kill of another state's
/// mover, in the `Cleared` state or outside a registered dungeon does nothing.
pub fn on_npc_killed(svc: &mut Services, ctx: &mut GameCtx<'_>, k: &KillCtx) {
    let Some((world, layer, prop)) = ctx
        .worlds
        .mover(k.victim)
        .filter(|m| !m.is_player())
        .map(|m| (m.core.world, m.core.layer, m.prop_id()))
    else {
        return;
    };
    if ctx.worlds.player(k.attacker).is_none() || !svc.event.instances.is_instance_world(world) {
        return;
    }
    let Some(party) = party_of_layer(layer) else {
        return;
    };
    let data = Arc::clone(ctx.data);
    let Some(def) = def_of(&data, world) else {
        return;
    };
    let Some(inst) = svc.event.instances.instances.get_mut(&(world, party)) else {
        return;
    };
    if !monsters_of(def, inst.state).any(|m| m.mover == prop) {
        return;
    }
    inst.kill_count += 1;
    if (inst.kill_count as usize) < count_of(def, inst.state) {
        return;
    }
    advance(svc, ctx, def, world, party, layer);
}

/// `SetNextState` with its recursion: from `Boss` the dungeon is `Cleared`; otherwise the state moves on, its monsters
/// appear, the players of the layer are moved when the new state has another teleport position, and a state without
/// monsters is skipped at once.
fn advance(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    def: &DungeonDef,
    world: WorldId,
    party: PartyId,
    layer: LayerId,
) {
    loop {
        let Some(inst) = svc.event.instances.instances.get_mut(&(world, party)) else {
            return;
        };
        if inst.state >= DungeonState::Boss {
            // the original keeps `nState = ID_BOSS` and logs 'C'; no reward, no audit line
            inst.state = DungeonState::Cleared;
            tracing::info!(world = world.0, party = party.0, "instance dungeon cleared");
            return;
        }
        let prev = teleport_of(def, inst.state);
        inst.state = inst.state.next();
        inst.kill_count = 0;
        let state = inst.state;
        spawn_state(ctx, def, world, layer, state);
        if let Some(pos) = teleport_of(def, state)
            && Some(pos) != prev
        {
            for p in players_in_layer(ctx, world, layer) {
                if let Err(e) = ctx
                    .worlds
                    .replace(p, world, pos, layer, ReplaceKind::Normal)
                {
                    tracing::warn!(
                        player = p.0,
                        "instance: the state teleport was refused: {e}"
                    );
                }
            }
        }
        // `if( pInfo->nKillCount >= GetObjCount(..) ) SetNextState`: a state without monsters is skipped
        if count_of(def, state) > 0 {
            return;
        }
    }
}

/// The players standing in or entering `(world, layer)`.
fn players_in_layer(ctx: &GameCtx<'_>, world: WorldId, layer: LayerId) -> Vec<ObjId> {
    let Some(l) = ctx.worlds.layer(world, layer) else {
        return Vec::new();
    };
    l.members
        .iter()
        .chain(l.queues.add.iter())
        .copied()
        .filter(|&o| ctx.worlds.store().is_player(o))
        .collect()
}
