//! Teleport and world change (Plan/15 §10, WP-WORLD-10; ref/06 §11; VR-082; F-085).
//!
//! [`Worlds::replace`] is `CMover::Replace` plus the bounds check of VR-082: it validates the target, stops the
//! mover and puts **one** entry per mover into the replace queue of its current layer (the last request of a
//! tick wins, 15 §10.2). The `_replace` phase (`Worlds::run_replace`) executes the entries:
//!
//! * **same world and layer** (§10.3): `SetPos{pos, stamp}` to the observers including the mover itself, the
//!   position is set (the cell and the visibility follow at the next `_modifylink`), the player's session is
//!   marked urgent;
//! * **another world or layer** (§10.4, players only): `DelObj` to the old observers, `Replace{world, pos}` to the
//!   player (urgent), the player leaves its old layer and enters the target layer's add queue; the target layer's
//!   `_add` then sends `SpawnSelf`, the spawns of the new view, `SmModeAll` and `Environment`. A change of the
//!   **world id** requests a `WorldChange` snapshot ([`Worlds::take_save_requests`]; `[save] on_world_change`).
//!
//! Nothing here flushes: the phase driver's fixed call point (`SpawnSink::urgent_flush` right after each layer's
//! `_replace`) sends the urgent session's queue, ending with `Replace`, as its own frame before anything of the
//! new world (the original `Notify()` right after `AddReplace`).
//!
//! Hooks of the other docs (guild-house cleanup, pet release, the GC1to1 reset: doc 19/21/22) are
//! [`WorldHooks::replace_leaving`](super::WorldHooks::replace_leaving) and
//! [`WorldHooks::world_change_entered`](super::WorldHooks::world_change_entered).

use super::Worlds;
use super::object::LinkState;
use super::phases::PhaseEnv;
use super::visibility::remove_from_view;
use crate::movement::intents::{intent_stamp, pb_vec};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_proto::{EncodedMsg, pb};
use flyff_sim::types::OBJACT_NONE;

/// `REPLACE_NORMAL` / `REPLACE_FORCE`: a normal replace is refused for a flying mover that is not a GM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceKind {
    /// `REPLACE_NORMAL`.
    Normal,
    /// `REPLACE_FORCE`.
    Force,
}

/// Why [`Worlds::replace`] refused a request (15 §10.1). Nothing is sent and nothing changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReplaceError {
    /// The mover is not in a world (not linked and not pending add) or does not exist.
    #[error("the mover is not in a world")]
    NotInWorld,
    /// The target world is `0`, `NULL_ID` or not loaded.
    #[error("the target world is not loaded")]
    WorldMissing,
    /// The target layer does not exist ("Layer not found").
    #[error("the target layer does not exist")]
    LayerMissing,
    /// VR-082: the target position is outside the world.
    #[error("the target position is outside the world")]
    OutOfWorld,
    /// `REPLACE_NORMAL` of a flying mover that is not a game master (silent in the original).
    #[error("a flying mover cannot be replaced normally")]
    FlyingNotAllowed,
}

/// One queued teleport (`REPLACEOBJ`, 15 §10.1): at most one per mover per layer queue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplaceEntry {
    /// The mover to move (`obj` of the plan).
    pub id: ObjId,
    /// The target world.
    pub world: WorldId,
    /// The target layer.
    pub layer: LayerId,
    /// The target position.
    pub pos: Vec3,
    /// Normal or forced.
    pub kind: ReplaceKind,
}

impl Worlds {
    /// `CMover::Replace` + VR-082 (15 §10.1), in this order:
    ///
    /// 1. the mover must be in a world (linked or pending add) -> [`ReplaceError::NotInWorld`]; the target world
    ///    must be loaded (`0` and `NULL_ID` never are) -> `WorldMissing`; the target layer must exist ->
    ///    `LayerMissing`;
    /// 2. **VR-082 (FIX)**: the position must be inside the target world -> `OutOfWorld` (the original sent
    ///    `SETPOS` for a same-world target and then silently ignored the position);
    /// 3. `Normal` and the mover is flying and not a GM -> `FlyingNotAllowed`;
    /// 4. the movement stops (`ClearDest`, no destination angle, no pending object action);
    /// 5. a pending `_modifylink` entry of the mover is dropped, and the one entry of the mover in the source
    ///    layer's replace queue is created or overwritten (target, position, layer and kind).
    pub fn replace(
        &mut self,
        mover: ObjId,
        world: WorldId,
        pos: Vec3,
        layer: LayerId,
        kind: ReplaceKind,
    ) -> Result<(), ReplaceError> {
        let core = self.store.core(mover).ok_or(ReplaceError::NotInWorld)?;
        if core.deleted || core.link == LinkState::Detached {
            return Err(ReplaceError::NotInWorld);
        }
        let (src_w, src_l) = (core.world, core.layer);
        if world.0 == 0 || world.0 == u32::MAX {
            return Err(ReplaceError::WorldMissing);
        }
        let target = self.world(world).ok_or(ReplaceError::WorldMissing)?;
        if !target.layers.contains_key(&layer) {
            return Err(ReplaceError::LayerMissing);
        }
        if !target.statics.in_world3(pos) {
            return Err(ReplaceError::OutOfWorld);
        }
        let m = self.store.mover(mover).ok_or(ReplaceError::NotInWorld)?;
        if kind == ReplaceKind::Normal && m.sim.act.flags.fly() {
            let gm = m.player().is_some_and(|p| p.ident.authority.is_gm());
            if !gm {
                return Err(ReplaceError::FlyingNotAllowed);
            }
        }
        if let Some(m) = self.store.mover_mut(mover) {
            // `ClearDest`, `OBJMSG_ACC_STOP` / `STOP_TURN` / `STAND` (doc 18 refines the act messages)
            m.sim.clear_dest();
            m.sim.clear_dest_angle();
            m.sim.cmd.act = OBJACT_NONE;
        }
        if let Some(c) = self.store.core_mut(mover) {
            c.pending_relink = false;
        }
        let Some(src) = self.layer_mut(src_w, src_l) else {
            return Err(ReplaceError::NotInWorld);
        };
        src.queues.modify_link.retain(|&x| x != mover);
        let entry = ReplaceEntry {
            id: mover,
            world,
            layer,
            pos,
            kind,
        };
        match src.queues.replace.iter_mut().find(|e| e.id == mover) {
            Some(e) => *e = entry,
            None => src.queues.replace.push(entry),
        }
        Ok(())
    }

    /// The players whose world id changed in the last `_replace` phases; the save pass turns each into a
    /// `WorldChange` snapshot request (`[save] on_world_change`, 15 §10.4 step 10).
    pub fn take_save_requests(&mut self) -> Vec<ObjId> {
        std::mem::take(&mut self.save_requests)
    }

    /// `_replace` of a layer (15 §10.3-§10.4): executes the queued entries in order. Entries of deleted or
    /// vanished movers are skipped; a non-player entry that changes the world or the layer is ignored.
    pub(crate) fn run_replace(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        let entries = match self.layer_mut(w, l) {
            Some(layer) => std::mem::take(&mut layer.queues.replace),
            None => return,
        };
        for e in entries {
            let Some(core) = self.store.core(e.id) else {
                continue;
            };
            if core.deleted || core.world != w || core.layer != l {
                continue;
            }
            if e.world == core.world && e.layer == core.layer {
                self.replace_same(e, env);
            } else if self.store.is_player(e.id) {
                self.replace_change(e, env);
            }
        }
    }

    /// 15 §10.3: the same world and layer.
    fn replace_same(&mut self, e: ReplaceEntry, env: &mut PhaseEnv<'_>) {
        let Some(statics) = self.statics_of(e.id).map(std::sync::Arc::clone) else {
            return;
        };
        // a teleport ends the NPC shop window (19 §7.2)
        if let Some(p) = self.store.mover_mut(e.id).and_then(|m| m.player_mut()) {
            p.econ.end_shop_session();
        }
        let mut pos = e.pos;
        if pos.y == 0.0 {
            pos.y = statics.full_height(Vec3::new(pos.x, 100.0, pos.z));
        }
        // the stamp is the start state at the frame the effect takes (after the frames of this tick)
        let frame = env.tick.frames_completed_after();
        let (stamp, viewers, is_player) = match self.store.mover(e.id) {
            Some(m) => (
                intent_stamp(&m.sim, frame),
                m.core.viewers.iter().copied().collect::<Vec<_>>(),
                m.is_player(),
            ),
            None => return,
        };
        let msg = EncodedMsg::new(
            e.id.0,
            pb::SetPos {
                pos: Some(pb_vec(pos)),
                stamp: Some(stamp),
            },
        );
        for v in viewers {
            env.sink.relay(v, &msg);
        }
        self.set_pos(e.id, pos);
        if is_player {
            env.sink.urgent(e.id);
        }
    }

    /// 15 §10.4: a world or layer change of a player.
    fn replace_change(&mut self, e: ReplaceEntry, env: &mut PhaseEnv<'_>) {
        let Some(core) = self.store.core(e.id) else {
            return;
        };
        let (old_w, old_l, link) = (core.world, core.layer, core.link);
        let Some(target) = self.world(e.world) else {
            return;
        };
        if !target.layers.contains_key(&e.layer) || !target.statics.in_world3(e.pos) {
            return; // validated at request time; the target vanished in between
        }
        let statics = std::sync::Arc::clone(&target.statics);
        // a world or layer change ends the NPC shop window (19 §7.2)
        if let Some(p) = self.store.mover_mut(e.id).and_then(|m| m.player_mut()) {
            p.econ.end_shop_session();
        }
        // 1, 3, 4 (docs 19/21/22): guild-house cleanup, pet release, the GC1to1 reset
        env.hooks
            .replace_leaving(&mut self.store, e.id, (old_w, old_l), (e.world, e.layer));
        // 2: DelObj to the old observers; `seen` and `viewers` are cleared
        remove_from_view(&mut self.store, env.sink, e.id);
        // 5: Replace to the player, urgent; the driver's call point flushes it right after this `_replace`
        let replace = EncodedMsg::new(
            0,
            pb::Replace {
                world_id: e.world.0,
                pos: Some(pb_vec(e.pos)),
            },
        );
        env.sink.relay(e.id, &replace);
        env.sink.urgent(e.id);
        // 6: unlink from the old layer
        if let LinkState::Linked { cell } = link
            && let Some(layer) = self.layer_mut(old_w, old_l)
        {
            layer.grid.remove(cell, e.id, true);
            layer.members.shift_remove(&e.id);
        }
        // the add-time clamp of `AddObj` (15 §5.3): a target below the terrain lands on it
        let mut pos = e.pos;
        let h = statics.land_height(pos.x, pos.z);
        if pos.y < h {
            pos.y = h;
        }
        if let Some(m) = self.store.mover_mut(e.id) {
            m.sim.pos = pos;
            m.core.pos = pos;
            m.core.wb_pos = pos;
            m.core.pos_changed = true;
            m.core.world = e.world;
            m.core.layer = e.layer;
            m.core.link = LinkState::PendingAdd;
            m.core.pending_relink = false;
            m.core.viewers.clear();
        }
        // 7: the target layer's `_add` sends `SpawnSelf` first
        self.pending_self.insert(e.id);
        if let Some(layer) = self.layer_mut(e.world, e.layer) {
            layer.queues.add.push(e.id);
        }
        // 10: only a change of the world id saves
        if e.world != old_w {
            self.save_requests.push(e.id);
        }
    }
}
