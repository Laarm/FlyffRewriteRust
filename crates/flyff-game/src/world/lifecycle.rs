//! Object lifecycle: spawn, delete, `set_pos` and the destroy order (Plan/15 §5.3-§5.5, WP-WORLD-02).
//!
//! Objects are created and queued by the `spawn_*` calls (id allocated at creation, `link = PendingAdd`),
//! linked into the grid at the next `_add` of their layer, relinked at `_modifylink` when their cell changed
//! and destroyed at `_delete` after [`Worlds::delete`] flagged them. The functions here never send a message:
//! visibility messages are produced by the phase code through the [`SpawnSink`](super::visibility::SpawnSink).

use super::Worlds;
use super::ctrl::Ctrl;
use super::grid::cell_of;
use super::item_drop::ItemDrop;
use super::object::{LinkState, ObjCore, ObjRef};
use super::phases::PhaseEnv;
use super::visibility::remove_from_view;
use crate::mover::Mover;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};

/// Why a spawn was refused (15 §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AddError {
    /// The world is not loaded.
    #[error("world is not loaded")]
    WorldMissing,
    /// The layer does not exist in the world.
    #[error("layer does not exist")]
    LayerMissing,
    /// The position is outside the world (`!in_world3`).
    #[error("position is outside the world")]
    OutOfWorld,
    /// The id space below `0xFF00_0000` is used up (fatal, 15 §5.2).
    #[error("object ids are exhausted")]
    IdsExhausted,
}

/// What the three spawn calls share: validation, the height clamp, id allocation, stamping and queueing.
struct Placement {
    pos: Vec3,
    id: ObjId,
}

impl Worlds {
    /// Validates the target and clamps `pos.y` to the land height (`CWorld::AddObj`,
    /// `src:_Common/World.cpp:396-485`): a world and layer must exist and `pos` must be inside the world; a
    /// dynamic object is lifted to the land height, **except** doors (they may be buried).
    fn place(
        &mut self,
        world: WorldId,
        layer: LayerId,
        core: &ObjCore,
        keep_y: bool,
    ) -> Result<Placement, AddError> {
        let w = self.world(world).ok_or(AddError::WorldMissing)?;
        if !w.layers.contains_key(&layer) {
            return Err(AddError::LayerMissing);
        }
        let mut pos = core.pos;
        if !w.statics.in_world3(pos) {
            return Err(AddError::OutOfWorld);
        }
        if !keep_y {
            let h = w.statics.land_height(pos.x, pos.z);
            if pos.y < h {
                pos.y = h;
            }
        }
        let id = self.store.alloc_id().ok_or(AddError::IdsExhausted)?;
        Ok(Placement { pos, id })
    }

    fn stamp(core: &mut ObjCore, world: WorldId, layer: LayerId, p: &Placement) {
        core.id = p.id;
        core.world = world;
        core.layer = layer;
        core.pos = p.pos;
        core.wb_pos = p.pos;
        core.link = LinkState::PendingAdd;
        core.pending_relink = false;
        core.pos_changed = false;
        core.deleted = false;
        core.viewers.clear();
    }

    fn queue_add(&mut self, world: WorldId, layer: LayerId, id: ObjId) {
        if let Some(l) = self.layer_mut(world, layer) {
            l.queues.add.push(id);
        }
    }

    /// Creates a mover in `(world, layer)`: validates the target, clamps the height, allocates the `ObjId`,
    /// stamps the header (and the sim's id and position), inserts it into the arena and queues it for the
    /// `_add` of its layer (15 §5.3). Nothing is sent before `_add`.
    pub fn spawn_mover(
        &mut self,
        world: WorldId,
        layer: LayerId,
        mut m: Mover,
    ) -> Result<ObjId, AddError> {
        let p = self.place(world, layer, &m.core, false)?;
        Self::stamp(&mut m.core, world, layer, &p);
        m.sim.id = p.id;
        m.sim.pos = p.pos;
        if let Some(pl) = m.player_mut() {
            pl.view.seen.clear();
        }
        let id = self.store.insert_mover(m);
        self.queue_add(world, layer, id);
        Ok(id)
    }

    /// Creates a ground item (see [`Worlds::spawn_mover`]).
    pub fn spawn_item(
        &mut self,
        world: WorldId,
        layer: LayerId,
        mut d: ItemDrop,
    ) -> Result<ObjId, AddError> {
        let p = self.place(world, layer, &d.core, false)?;
        Self::stamp(&mut d.core, world, layer, &p);
        let id = self.store.insert_item(d);
        self.queue_add(world, layer, id);
        Ok(id)
    }

    /// Creates a ctrl (see [`Worlds::spawn_mover`]); a door keeps its height.
    pub fn spawn_ctrl(
        &mut self,
        world: WorldId,
        layer: LayerId,
        mut c: Ctrl,
    ) -> Result<ObjId, AddError> {
        let p = self.place(world, layer, &c.core, c.door)?;
        Self::stamp(&mut c.core, world, layer, &p);
        let id = self.store.insert_ctrl(c);
        self.queue_add(world, layer, id);
        Ok(id)
    }

    /// `CObj::Delete`: flags the object and queues it for the `_delete` of its layer. Idempotent. A
    /// pending-add object is skipped by `_add` and removed without any message at `_delete`.
    pub fn delete(&mut self, id: ObjId) {
        let Some(core) = self.store.core_mut(id) else {
            return;
        };
        if core.deleted {
            return;
        }
        core.deleted = true;
        let (w, l) = (core.world, core.layer);
        match self.layer_mut(w, l) {
            Some(layer) => layer.queues.delete.push(id),
            None => {
                // the layer is gone: nothing can link or show it, remove it at once
                self.store.remove(id);
            }
        }
    }

    /// `CObj::SetPos` (server branch, `src:_Common/Obj.cpp:532-655`; 15 §5.5).
    ///
    /// * a position equal to the current one changes nothing;
    /// * `pos_changed` is set (region processing);
    /// * a position outside the world is silently ignored for a pending or linked object;
    /// * a mover's **sim position is written first**, then the `core.pos` copy;
    /// * a move into another cell queues exactly one relink (`pending_relink`): the grid and the visibility
    ///   sets change at `_modifylink` of the same tick.
    pub fn set_pos(&mut self, id: ObjId, p: Vec3) {
        let Some(core) = self.store.core(id) else {
            return;
        };
        if p == core.pos {
            return;
        }
        let (w, l, link) = (core.world, core.layer, core.link);
        if let Some(c) = self.store.core_mut(id) {
            c.pos_changed = true;
        }
        let attached = !matches!(link, LinkState::Detached);
        let geom = self
            .world(w)
            .map(|x| (x.geom, x.statics.in_world(p.x, p.z)));
        if attached && geom.is_some_and(|(_, inside)| !inside) {
            return;
        }
        if let Some(m) = self.store.mover_mut(id) {
            m.sim.pos = p;
        }
        let relink = {
            let Some(c) = self.store.core_mut(id) else {
                return;
            };
            c.pos = p;
            c.wb_pos = p;
            match (c.link, geom) {
                (LinkState::Linked { cell }, Some((g, _)))
                    if cell_of(&g, p) != cell && !c.pending_relink =>
                {
                    c.pending_relink = true;
                    true
                }
                _ => false,
            }
        };
        if relink && let Some(layer) = self.layer_mut(w, l) {
            layer.queues.modify_link.push(id);
        }
    }

    /// The destroy order of 15 §5.4 / `CWorld::_delete` + the destructors (ref/06 §7.6):
    ///
    /// 1. a pending-add object leaves the add queue (no view work);
    /// 2. it leaves its grid cell and the layer's member set;
    /// 3. `remove_from_view`: `DelObj` to every viewer except itself; a player leaves the viewer set of
    ///    everything it saw;
    /// 4. unregister: the NPC-menu hook (`NpcChecker::on_npc_removed`) for NPCs registered at `_add`;
    /// 5. respawn bookkeeping: the layer's [`Respawner`](super::Respawner) counters (and the restart of a giant
    ///    line's timer), then the `on_destroyed` hook (`aggressive` = the monster's active-attack flag);
    /// 6. removal from the arena and the id index (the id stays burned).
    pub(crate) fn destroy(
        &mut self,
        world: WorldId,
        layer: LayerId,
        id: ObjId,
        env: &mut PhaseEnv<'_>,
    ) {
        let Some(core) = self.store.core(id) else {
            return;
        };
        let (link, respawn, is_player) = (core.link, core.respawn, self.store.is_player(id));
        // 1.
        if link == LinkState::PendingAdd
            && let Some(l) = self.layer_mut(world, layer)
        {
            l.queues.add.retain(|&x| x != id);
        }
        // 2.
        let registered_npc = matches!(link, LinkState::Linked { .. })
            && self
                .store
                .mover(id)
                .and_then(|m| m.npc())
                .is_some_and(|n| !n.menus.is_empty());
        if let LinkState::Linked { cell } = link
            && let Some(l) = self.layer_mut(world, layer)
        {
            l.grid.remove(cell, id, is_player);
            l.members.shift_remove(&id);
        }
        if let Some(c) = self.store.core_mut(id) {
            c.link = LinkState::Detached;
        }
        // 2b. a ctrl leaves its layer's collider set and key map (15 §14.6-§14.7)
        if matches!(self.store.lookup(id), Some(ObjRef::Ctrl(_))) {
            self.ctrl_removed(world, layer, id);
        }
        // 3.
        remove_from_view(&mut self.store, env.sink, id);
        // 4.
        if registered_npc {
            self.npc_checker.on_npc_removed(id);
            env.hooks.on_npc_removed(id);
        }
        // 5.
        if let Some(origin) = respawn {
            let aggressive = self
                .store
                .mover(id)
                .and_then(|m| m.monster())
                .is_some_and(|m| m.active_attack);
            self.respawn_on_destroy(world, layer, origin, aggressive);
            env.hooks.on_destroyed(world, layer, origin, aggressive);
        }
        // 6.
        self.pending_self.remove(&id);
        if self.store.remove(id) {
            self.stats.destroyed += 1;
        }
    }
}
