//! The per-tick world phases (Plan/15 §5.3, 01 §6.3, 31 §3.4, WP-WORLD-02).
//!
//! For each world in ascending id and each of its layers in ascending id, one tick runs
//!
//! ```text
//! _add -> _process -> _modifylink -> _OnDie -> _replace -> _delete        (then, after the last layer: layer GC)
//! ```
//!
//! `_process` is **frame-major** (31 §3.4): the sleep rule picks the `awake` movers, the per-tick AI head runs
//! for each of them, then for each of the 4 frames `F = 4n .. 4n+3` the frame snapshot is captured, every awake
//! mover is stepped (followed by the write-back of `core.pos` / `core.angle` from its `SimMover`) and the
//! BARRIER drains the events; finally every awake mover runs its tick tail. Items and ctrls are processed after
//! the movers whether or not anybody sees them.
//!
//! The per-mover bodies (AI, `step_frame`, event handlers, tick tails, `_OnDie`, `_replace`) are not here: they
//! plug in through the [`WorldHooks`] trait, whose defaults do nothing. The phase driver alone moves
//! `core.pos` / `core.angle` during phase 2, always as a copy of the sim position (15 §5.5).

use super::Worlds;
use super::grid::{Grid, cell_of};
use super::layer::OnDieEntry;
use super::object::{LinkState, MoverKey, ObjRef, ObjectStore, RespawnOrigin};
use super::replace::ReplaceKind;
use super::visibility::{Phase, SpawnSink, add_to_view, modify_view};
use crate::movement::MovementState;
use crate::rng::RngSet;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::rt::{Telemetry, WorldPhase};
use flyff_common::{GameMs, LayerId, ObjId, SimFrame, Tick, WorldId};
use flyff_data::types::MenuId;
use flyff_data::worlds::RegionDef;
use flyff_proto::EncodedMsg;
use flyff_rules::consts::defs::neuz::AII_PET;
use std::sync::Arc;

use super::catalog::WorldStatic;

/// What a per-mover hook may touch while a layer is processed: the object store (all arenas), the static data
/// of the world, the clock. Objects cannot be created or destroyed directly; [`FrameCtx::delete`] queues a
/// delete that the driver executes right after the hook returns (the `_delete` phase destroys it).
pub struct FrameCtx<'a> {
    /// All objects. Movers are addressed by their [`MoverKey`] (`store.movers`).
    pub store: &'a mut ObjectStore,
    /// The static data of the world (collision, regions, bounds).
    pub statics: &'a Arc<WorldStatic>,
    /// The world being processed.
    pub world: WorldId,
    /// The layer being processed.
    pub layer: LayerId,
    /// The tick `n` (frames `4n .. 4n+3`).
    pub tick: Tick,
    /// The game clock of this tick.
    pub now: GameMs,
    /// The movement pipeline's shared data and counters (18 §3, §17).
    pub movement: &'a mut MovementState,
    /// The link-map grid of the layer being processed (the target scans of the AI walk its player lists in
    /// the legacy order, 18 §8.7).
    pub grid: &'a Grid,
    /// The dynamic `OT_CTRL` colliders of the layer being processed (15 §14.7): every sim call of its movers
    /// tests them before the statics.
    pub dynamic: &'a flyff_sim::collision::dynamic::DynamicColliders,
    /// Where the pipeline's messages go: `sink.relay(player, msg)` pushes one encoded message into a player's
    /// session (the visibility messages of the same tick went through the same sink, so the order of a
    /// player's batch is the order of the calls).
    pub sink: &'a mut dyn SpawnSink,
    /// The world's random stream (`RngSet::world(world)`, 18 §8.5), shared by every layer of the world.
    pub rng: &'a mut dyn GameRng,
    deletes: Vec<ObjId>,
    spawns: Vec<crate::ai::MonsterSpawn>,
    replaces: Vec<(ObjId, WorldId, LayerId, Vec3, ReplaceKind)>,
}

impl FrameCtx<'_> {
    /// `CObj::Delete`: the object is flagged and queued after the hook returns.
    pub fn delete(&mut self, id: ObjId) {
        self.deletes.push(id);
    }

    /// `true` when `id` was handed to [`FrameCtx::delete`] during this hook call: the object is flagged for
    /// deletion only after the hook returns, but the original's `CObj::Delete` is immediate, so a scan that
    /// follows a pickup in the same call (the eat pet's `SubItemLoot`) must skip it.
    pub fn is_deleting(&self, id: ObjId) -> bool {
        self.deletes.contains(&id)
    }

    /// `ADDOBJ` of a monster the AI creates (a summon, a transformation): queued, created right after the hook
    /// returns and linked by the `_add` of its layer on the next tick (18 §8.4 `AiCmd::Spawn`).
    pub fn spawn(&mut self, s: crate::ai::MonsterSpawn) {
        self.spawns.push(s);
    }

    /// `REPLACE(mover, world, pos, kind, layer)` requested from a hook (a ctrl's teleport, 15 §14.3): queued and
    /// executed with [`Worlds::replace`] right after the hook returns. A refused request (the target is not
    /// loaded, the position is outside the world, a flying mover) is dropped silently like the original's.
    pub fn replace(
        &mut self,
        mover: ObjId,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
        kind: ReplaceKind,
    ) {
        self.replaces.push((mover, world, layer, pos, kind));
    }
}

/// The plug points of the world driver (doc 15 §5.3; docs 16/17/18/20/21 implement them). Every method has a
/// no-op default, so a world runs without any of them.
pub trait WorldHooks: Send {
    /// A phase of a layer starts (tests record the phase order).
    fn on_phase(&mut self, _world: WorldId, _layer: LayerId, _phase: Phase) {}

    // ---- `_add` ----

    /// Doc 18 `init_ai` for an object that was just linked.
    fn init_ai(&mut self, _store: &mut ObjectStore, _id: ObjId) {}

    /// Doc 20 `NpcChecker::on_npc_added(id, world, layer, menus)`: called exactly once at `_add` for an NPC
    /// whose `NpcDef.menus` is non-empty.
    fn on_npc_added(&mut self, _id: ObjId, _world: WorldId, _layer: LayerId, _menus: &[MenuId]) {}

    // ---- `_process` (31 §3.4 frame-major) ----

    /// The AI pass of the layer starts: `awake` are the movers the sleep rule lets run (slot order). Called once
    /// before the first [`WorldHooks::ai`] of the layer (18 §3.3: the AI sees the start-of-tick state).
    fn ai_begin(&mut self, _ctx: &mut FrameCtx<'_>, _awake: &[MoverKey]) {}

    /// The per-tick head (AI pass) of one awake mover, called in snapshot order before any frame.
    fn ai(&mut self, _ctx: &mut FrameCtx<'_>, _mover: MoverKey) {}

    /// Start of frame `frame` before any mover steps it: capture the frame snapshot of the awake movers.
    fn snapshot(&mut self, _ctx: &mut FrameCtx<'_>, _frame: SimFrame, _awake: &[MoverKey]) {}

    /// Steps one awake mover by frame `frame` (`flyff_sim::step_frame`). The driver copies the sim position
    /// and yaw into `core` afterwards.
    fn step(&mut self, _ctx: &mut FrameCtx<'_>, _frame: SimFrame, _mover: MoverKey) {}

    /// The BARRIER of frame `frame`: drain the events of the awake movers in order; their effects apply from
    /// `frame + 1`.
    fn barrier(&mut self, _ctx: &mut FrameCtx<'_>, _frame: SimFrame, _awake: &[MoverKey]) {}

    /// The tick tail of one awake mover (regeneration, buffs, region processing of players).
    fn tail(&mut self, _ctx: &mut FrameCtx<'_>, _mover: MoverKey) {}

    /// `CCommonCtrl::Process` of a **psychic wall** (doc 17; the common ctrls run in the driver, WP-WORLD-18).
    fn process_ctrl(&mut self, _ctx: &mut FrameCtx<'_>, _ctrl: super::object::CtrlKey) {}

    /// The layer hooks after the objects were processed (doc 17 `cont_damage::process_layer`).
    fn layer_process(&mut self, _ctx: &mut FrameCtx<'_>) {}

    // ---- `_OnDie`, `_replace` ----

    /// Doc 21 drains the queued deaths of the layer.
    fn on_die(
        &mut self,
        _worlds: &mut Worlds,
        _world: WorldId,
        _layer: LayerId,
        _entries: Vec<OnDieEntry>,
    ) {
    }

    /// Extra `_replace` work of other docs, after the driver executed the layer's `queues.replace` (15 §10; the
    /// teleport itself is `world::replace`).
    fn replace(&mut self, _worlds: &mut Worlds, _world: WorldId, _layer: LayerId) {}

    /// A player leaves its layer for another world or layer (15 §10.4 steps 1, 3, 4): doc 22's guild-house
    /// cleanup, doc 19's pet release and eat-pet deactivation, doc 21's GC1to1 state reset. Called before the
    /// player's `DelObj` / `Replace` messages.
    fn replace_leaving(
        &mut self,
        _store: &mut ObjectStore,
        _player: ObjId,
        _from: (WorldId, LayerId),
        _to: (WorldId, LayerId),
    ) {
    }

    /// A player that changed world or layer was linked into its target layer and got `SpawnSelf` and the spawns
    /// of its new view (15 §10.4 steps 8-9, at the target `_add`): doc 16's `SmModeAll`, doc 22's guild-house
    /// DST and housing papering, doc 19's eat-pet re-summon. `ctx.sink.relay` writes into the player's session.
    fn world_change_entered(&mut self, _ctx: &mut FrameCtx<'_>, _player: MoverKey) {}

    /// The region attribute of a player changed class (`IsRegionMove`, 15 §11.4): doc 17 cancels duels.
    fn region_class_changed(
        &mut self,
        _store: &mut ObjectStore,
        _player: ObjId,
        _old: u32,
        _new: u32,
    ) {
    }

    /// Doc 22's `IsInstanceDungeon(world)`: portals into such a world go through [`WorldHooks::instance_enter`].
    fn is_instance_world(&self, _world: WorldId) -> bool {
        false
    }

    /// Doc 22's `EnteranceDungeon(player, region.tele_world)`: a player walked into an instance portal from a
    /// normal world.
    fn instance_enter(&mut self, _store: &mut ObjectStore, _player: ObjId, _region: &RegionDef) {}

    // ---- `_delete` ----

    /// Doc 20 `NpcChecker::on_npc_removed(id)` for an NPC registered at `_add`.
    fn on_npc_removed(&mut self, _id: ObjId) {}

    /// WP-WORLD-14: the respawn counter of the object's spawn line (15 §5.4 step 5).
    fn on_destroyed(
        &mut self,
        _world: WorldId,
        _layer: LayerId,
        _origin: RespawnOrigin,
        _aggressive: bool,
    ) {
    }

    // ---- after the last layer of a world ----

    /// WP-WORLD-07 layer garbage collection (15 §4.3).
    fn layer_gc(&mut self, _worlds: &mut Worlds, _world: WorldId) {}
}

/// A [`WorldHooks`] that does nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHooks;

impl WorldHooks for NoHooks {}

/// What one run of the phases needs besides the worlds.
pub struct PhaseEnv<'a> {
    /// The tick `n`.
    pub tick: Tick,
    /// The game clock of the tick.
    pub now: GameMs,
    /// Receives every spawn / despawn of the visibility sets and the urgent-flush point.
    pub sink: &'a mut dyn SpawnSink,
    /// The per-mover bodies.
    pub hooks: &'a mut dyn WorldHooks,
    /// The random streams (the hooks get the one of the world being processed).
    pub rng: &'a mut RngSet,
}

/// The first of the four frames of a tick.
fn first_frame(tick: Tick) -> u64 {
    tick.0.saturating_mul(4)
}

/// The lap timer of the world frame (WP-OPS-10, Plan/10 §12.2): after each sub-phase of a layer it reports the time
/// since the previous lap to `Telemetry::record_phase`, which sums over the layers of the tick. Unarmed (no
/// telemetry) it does nothing, so the unit tests of the phase driver construct and pay nothing. The clock is the
/// telemetry's own monotonic one: game code never reads `Instant`.
#[derive(Debug, Default)]
pub struct PhaseTimes {
    telemetry: Option<Arc<Telemetry>>,
    last_ns: u64,
}

impl PhaseTimes {
    /// Arms the timer once (a later call keeps the telemetry it already has).
    pub fn arm(&mut self, telemetry: &Arc<Telemetry>) {
        if self.telemetry.is_none() {
            self.telemetry = Some(Arc::clone(telemetry));
        }
    }

    /// `true` once armed.
    pub fn is_armed(&self) -> bool {
        self.telemetry.is_some()
    }

    /// Starts the first lap.
    fn start(&mut self) {
        if let Some(t) = &self.telemetry {
            self.last_ns = t.now_ns();
        }
    }

    /// Ends the lap of `phase` and starts the next one.
    fn lap(&mut self, phase: WorldPhase) {
        if let Some(t) = &self.telemetry {
            let now = t.now_ns();
            t.record_phase(phase, now.saturating_sub(self.last_ns));
            self.last_ns = now;
        }
    }
}

/// Names the sub-phase in the panic context: a panic inside it is logged and audited with that phase.
fn in_phase(phase: WorldPhase) {
    crate::panic::set_unit(None, None, phase.name());
}

impl Worlds {
    /// One tick of every world (15 §5.3): per world ascending, per existing layer ascending, the six phases,
    /// then the layer GC hook. A world or layer created or released by a hook during the tick is picked up
    /// (or skipped) at its turn.
    pub fn run_tick(&mut self, env: &mut PhaseEnv<'_>) {
        let world_ids: Vec<WorldId> = self.world_ids().collect();
        self.phase_times.start();
        for w in world_ids {
            let layer_ids: Vec<LayerId> = match self.world(w) {
                Some(world) => world.layers.keys().copied().collect(),
                None => continue,
            };
            for l in layer_ids {
                if self.layer(w, l).is_none() {
                    continue;
                }
                in_phase(WorldPhase::Add);
                self.phase_add(w, l, env);
                self.phase_times.lap(WorldPhase::Add);
                in_phase(WorldPhase::Process);
                self.phase_process(w, l, env);
                self.phase_times.lap(WorldPhase::Process);
                in_phase(WorldPhase::ModifyLink);
                self.phase_modify_link(w, l, env);
                self.phase_times.lap(WorldPhase::ModifyLink);
                in_phase(WorldPhase::OnDie);
                self.phase_on_die(w, l, env);
                self.phase_times.lap(WorldPhase::OnDie);
                in_phase(WorldPhase::Replace);
                self.phase_replace(w, l, env);
                self.phase_times.lap(WorldPhase::Replace);
                in_phase(WorldPhase::Delete);
                self.phase_delete(w, l, env);
                self.phase_times.lap(WorldPhase::Delete);
            }
            in_phase(WorldPhase::LayerGc);
            self.layer_gc(w, env);
            env.hooks.layer_gc(self, w);
            self.phase_times.lap(WorldPhase::LayerGc);
        }
        crate::panic::clear_unit();
    }

    fn begin(&self, w: WorldId, l: LayerId, phase: Phase, env: &mut PhaseEnv<'_>) {
        env.hooks.on_phase(w, l, phase);
        env.sink.phase_begin(phase);
    }

    /// `_add`: link the queued objects into their cell, give players their own viewer entry, run the init
    /// hooks and `add_to_view`.
    pub(crate) fn phase_add(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        self.begin(w, l, Phase::Add, env);
        let queue = match self.layer_mut(w, l) {
            Some(layer) => std::mem::take(&mut layer.queues.add),
            None => Vec::new(),
        };
        for id in queue {
            let Some(core) = self.store.core(id) else {
                continue;
            };
            // flagged for deletion: left pending, removed without any message at `_delete`
            if core.deleted
                || core.link != LinkState::PendingAdd
                || core.world != w
                || core.layer != l
            {
                continue;
            }
            let is_player = self.store.is_player(id);
            let Some(world) = self.world(w) else { continue };
            let cell = cell_of(&world.geom, core.pos);
            let linked = match self.layer_mut(w, l) {
                Some(layer) => {
                    if layer.grid.insert(cell, id, is_player) {
                        layer.members.insert(id);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            };
            if !linked {
                tracing::error!(
                    ?id,
                    ?cell,
                    "object position has no grid cell: object dropped"
                );
                self.delete(id);
                continue;
            }
            if let Some(c) = self.store.core_mut(id) {
                c.link = LinkState::Linked { cell };
                if is_player {
                    c.viewers.insert(id); // a linked player is in its own viewer set (V2)
                }
            }
            if let Some(p) = self.store.mover_mut(id).and_then(|m| m.player_mut()) {
                p.view.seen.clear();
            }
            let world_change = self.pending_self.remove(&id);
            // a ctrl registers its collider and its key in the layer (15 §14.6-§14.7, before the viewers learn it)
            if matches!(self.store.lookup(id), Some(ObjRef::Ctrl(_))) {
                self.ctrl_added(w, l, id);
            }
            env.hooks.init_ai(&mut self.store, id);
            let menus = self
                .store
                .mover(id)
                .and_then(|m| m.npc())
                .filter(|n| !n.menus.is_empty())
                .map(|n| n.menus.clone());
            if let Some(menus) = menus {
                self.npc_checker.on_npc_added(id, w, l, &menus); // VR-012/013 index (doc 20 §8.1)
                env.hooks.on_npc_added(id, w, l, &menus);
            }
            if world_change {
                // 15 §10.4 step 7: the player's own spawn comes first, then the spawns of the new view
                env.sink.spawn_self(&self.store, id);
            }
            if let Some(layer) = self.worlds.get(&w).and_then(|x| x.layers.get(&l)) {
                add_to_view(&layer.grid, &mut self.store, env.sink, id);
            }
            if world_change {
                // steps 8-9: `SmModeAll` (doc 16, message not in the protocol slice yet), `Environment`, the
                // hooks of docs 19/22
                if let Some(sm) = self.store.mover(id).and_then(crate::stats::sm::sm_mode_all) {
                    env.sink.relay(id, &EncodedMsg::new(0, sm));
                }
                let env_msg = EncodedMsg::new(0, super::environment::environment_msg(self));
                env.sink.relay(id, &env_msg);
                if let Some(key) = self.store.mover_key(id) {
                    self.with_ctx_opt(w, l, env, |ctx, hooks| hooks.world_change_entered(ctx, key));
                }
            }
        }
        env.sink.phase_end(Phase::Add);
    }

    /// `_process`: the frame-major pass over the awake movers, then items and ctrls.
    pub(crate) fn phase_process(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        self.begin(w, l, Phase::Process, env);
        let members: Vec<ObjId> = match self.layer(w, l) {
            Some(layer) => layer.members.iter().copied().collect(),
            None => Vec::new(),
        };
        let Some(statics) = self.world(w).map(|x| Arc::clone(&x.statics)) else {
            env.sink.phase_end(Phase::Process);
            return;
        };

        // the sleep rule (15 §5.6): unobserved non-pet movers do not run
        let mut awake: Vec<MoverKey> = Vec::new();
        let mut items: Vec<ObjId> = Vec::new();
        let mut ctrls: Vec<super::object::CtrlKey> = Vec::new();
        for &id in &members {
            match self.store.lookup(id) {
                Some(ObjRef::Mover(k)) => {
                    if let Some(m) = self.store.movers.get(k)
                        && !m.core.deleted
                    {
                        let pet = m.monster().is_some_and(|x| x.ai.kind == AII_PET);
                        if m.is_player() || !m.core.viewers.is_empty() || pet {
                            awake.push(k);
                        } else {
                            self.stats.slept += 1;
                        }
                    }
                }
                Some(ObjRef::Item(_)) => items.push(id),
                Some(ObjRef::Ctrl(k)) => ctrls.push(k),
                None => {}
            }
        }

        // (a) the AI pass
        self.with_ctx(w, l, &statics, env, |ctx, hooks| {
            hooks.ai_begin(ctx, &awake)
        });
        for &k in &awake {
            self.with_ctx(w, l, &statics, env, |ctx, hooks| hooks.ai(ctx, k));
        }
        // (b) four frames, frame-major
        for i in 0..4_u64 {
            let frame = SimFrame(first_frame(env.tick) + i);
            self.with_ctx(w, l, &statics, env, |ctx, hooks| {
                hooks.snapshot(ctx, frame, &awake)
            });
            for &k in &awake {
                if self.store.movers.get(k).is_none_or(|m| m.core.deleted) {
                    continue;
                }
                self.with_ctx(w, l, &statics, env, |ctx, hooks| hooks.step(ctx, frame, k));
                self.write_back(w, l, k);
            }
            self.with_ctx(w, l, &statics, env, |ctx, hooks| {
                hooks.barrier(ctx, frame, &awake)
            });
        }
        // (c) the tick tails
        for &k in &awake {
            if self.store.movers.get(k).is_none_or(|m| m.core.deleted) {
                continue;
            }
            self.with_ctx(w, l, &statics, env, |ctx, hooks| hooks.tail(ctx, k));
            // region attribute, portals (15 §11.2-§11.3)
            self.process_region(w, l, k, env);
        }
        // items: expiry runs whether or not anybody sees the drop (15 §16.2)
        for id in items {
            let now = env.now;
            if self
                .store
                .item(id)
                .is_some_and(|d| !d.core.deleted && d.expired(now))
            {
                self.delete(id);
            }
        }
        // ctrls: the common ones run here (WP-WORLD-18), the psychic wall is doc 17's
        for k in ctrls {
            let wall = self
                .store
                .ctrls
                .get(k)
                .is_some_and(|c| matches!(c.special, super::ctrl::CtrlSpecial::PsychicWall(_)));
            if wall {
                self.with_ctx(w, l, &statics, env, |ctx, hooks| hooks.process_ctrl(ctx, k));
            } else {
                self.process_ctrl(w, l, k, &statics, env);
            }
        }
        self.with_ctx(w, l, &statics, env, |ctx, hooks| hooks.layer_process(ctx));
        env.sink.phase_end(Phase::Process);
    }

    /// [`Worlds::with_ctx`] for a caller that has no `statics` at hand: looks the world's statics up (does
    /// nothing when the world is gone).
    fn with_ctx_opt(
        &mut self,
        w: WorldId,
        l: LayerId,
        env: &mut PhaseEnv<'_>,
        f: impl FnOnce(&mut FrameCtx<'_>, &mut dyn WorldHooks),
    ) {
        let Some(statics) = self.world(w).map(|x| Arc::clone(&x.statics)) else {
            return;
        };
        self.with_ctx(w, l, &statics, env, f);
    }

    /// Runs `f` with a [`FrameCtx`] and executes the deletes and the monster spawns it queued.
    fn with_ctx(
        &mut self,
        w: WorldId,
        l: LayerId,
        statics: &Arc<WorldStatic>,
        env: &mut PhaseEnv<'_>,
        f: impl FnOnce(&mut FrameCtx<'_>, &mut dyn WorldHooks),
    ) {
        // the grid of the layer (a layer that vanished has nothing to process)
        let Some((grid, dynamic)) = self
            .worlds
            .get(&w)
            .and_then(|x| x.layers.get(&l))
            .map(|layer| (&layer.grid, &*layer.dyn_colliders))
        else {
            return;
        };
        let (deletes, spawns, replaces) = {
            let mut ctx = FrameCtx {
                store: &mut self.store,
                statics,
                world: w,
                layer: l,
                tick: env.tick,
                now: env.now,
                movement: &mut self.movement,
                grid,
                dynamic,
                sink: &mut *env.sink,
                rng: env.rng.world(w),
                deletes: Vec::new(),
                spawns: Vec::new(),
                replaces: Vec::new(),
            };
            f(&mut ctx, &mut *env.hooks);
            (ctx.deletes, ctx.spawns, ctx.replaces)
        };
        for (mover, world, layer, pos, kind) in replaces {
            // refused requests are silent (`REPLACE` returns without a message)
            let _ = self.replace(mover, world, pos, layer, kind);
        }
        for id in deletes {
            self.delete(id);
        }
        if !spawns.is_empty() {
            let data = Arc::clone(&self.movement.shared.data);
            for s in spawns {
                crate::ai::spawn::commit(self, &data, env.rng.world(w), &s);
            }
        }
    }

    /// The write-back after `step` (15 §5.5): `core.angle` / `core.angle_x` copy the sim, `core.pos` copies the
    /// sim position through the `set_pos` rules (an out-of-world sim position is refused and the sim is put
    /// back), a cell change queues exactly one relink. A `core.pos` that differs from the previous write-back
    /// value was written by somebody else during phase 2: counted and logged.
    fn write_back(&mut self, w: WorldId, l: LayerId, key: MoverKey) {
        let Some(world) = self.world(w) else {
            return;
        };
        let geom = world.geom;
        let statics = Arc::clone(&world.statics);
        let Some(m) = self.store.movers.get_mut(key) else {
            return;
        };
        self.stats.write_backs += 1;
        if m.core.pos != m.core.wb_pos {
            self.stats.direct_core_writes += 1;
            tracing::error!(
                id = ?m.core.id,
                core = ?m.core.pos,
                last = ?m.core.wb_pos,
                "core.pos was written directly during phase 2 (15 §5.5): only the sim moves a mover"
            );
        }
        m.core.angle = m.sim.angle;
        m.core.angle_x = m.sim.angle_x;
        let np = m.sim.pos;
        let mut relink = None;
        if np != m.core.pos {
            m.core.pos_changed = true;
            if statics.in_world(np.x, np.z) {
                m.core.pos = np;
                if let LinkState::Linked { cell } = m.core.link
                    && cell_of(&geom, np) != cell
                    && !m.core.pending_relink
                {
                    m.core.pending_relink = true;
                    relink = Some(m.core.id);
                }
            } else {
                // `SetPos` ignores a position outside the world: the mover stays where it was
                m.sim.pos = m.core.pos;
                self.stats.rejected_moves += 1;
            }
        }
        m.core.wb_pos = m.core.pos;
        if let Some(id) = relink
            && let Some(layer) = self.layer_mut(w, l)
        {
            layer.queues.modify_link.push(id);
        }
    }

    /// `_modifylink`: move the objects whose cell changed in the grid and update their visibility.
    pub(crate) fn phase_modify_link(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        self.begin(w, l, Phase::ModifyLink, env);
        let queue = match self.layer_mut(w, l) {
            Some(layer) => std::mem::take(&mut layer.queues.modify_link),
            None => Vec::new(),
        };
        for id in queue {
            let Some(core) = self.store.core_mut(id) else {
                continue;
            };
            core.pending_relink = false;
            if core.deleted || core.world != w || core.layer != l {
                continue;
            }
            let LinkState::Linked { cell: old } = core.link else {
                continue;
            };
            let pos = core.pos;
            let is_player = self.store.is_player(id);
            let Some(world) = self.world_mut(w) else {
                continue;
            };
            let new = cell_of(&world.geom, pos);
            if new == old {
                continue;
            }
            let Some(layer) = world.layers.get_mut(&l) else {
                continue;
            };
            layer.grid.remove(old, id, is_player);
            if !layer.grid.insert(new, id, is_player) {
                // cannot happen for an in-world position; keep the object where it was
                layer.grid.insert(old, id, is_player);
                continue;
            }
            if let Some(c) = self.store.core_mut(id) {
                c.link = LinkState::Linked { cell: new };
            }
            self.stats.relinks += 1;
            if let Some(layer) = self.worlds.get(&w).and_then(|x| x.layers.get(&l)) {
                modify_view(&layer.grid, &mut self.store, env.sink, id, old, new);
            }
        }
        env.sink.phase_end(Phase::ModifyLink);
    }

    /// `_OnDie`: the queued deaths go to doc 21.
    pub(crate) fn phase_on_die(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        self.begin(w, l, Phase::OnDie, env);
        let entries = match self.layer_mut(w, l) {
            Some(layer) => std::mem::take(&mut layer.queues.on_die),
            None => Vec::new(),
        };
        if !entries.is_empty() {
            env.hooks.on_die(self, w, l, entries);
        }
        env.sink.phase_end(Phase::OnDie);
    }

    /// `_replace`: executed by the world-flow hook; then the fixed urgent-flush point.
    pub(crate) fn phase_replace(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        self.begin(w, l, Phase::Replace, env);
        self.run_replace(w, l, env);
        env.hooks.replace(self, w, l);
        env.sink.phase_end(Phase::Replace);
        env.sink.urgent_flush();
    }

    /// `_delete`: destroy the queued objects in order (15 §5.4).
    pub(crate) fn phase_delete(&mut self, w: WorldId, l: LayerId, env: &mut PhaseEnv<'_>) {
        self.begin(w, l, Phase::Delete, env);
        let queue = match self.layer_mut(w, l) {
            Some(layer) => std::mem::take(&mut layer.queues.delete),
            None => Vec::new(),
        };
        for id in queue {
            self.destroy(w, l, id, env);
        }
        env.sink.phase_end(Phase::Delete);
    }
}
