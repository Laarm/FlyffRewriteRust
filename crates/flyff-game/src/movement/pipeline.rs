//! [`MovementHooks`]: the movement pipeline as the [`WorldHooks`] of phase 2 (Plan/18 §3.3, 31 §3.4;
//! WP-AI-01).
//!
//! The world driver calls, per layer and tick `n`:
//!
//! ```text
//! ai_begin(awake)                    capture the start-of-tick snapshot for the AI
//! ai(m) for m in awake               stale pending intents dropped, the AI pass (intents stamped 4n)
//! for F in 4n .. 4n+3:
//!   snapshot(F, awake)               the start-of-frame snapshot (cross-mover reads, 31 §3.3)
//!   step(F, m) for m in awake        capture_pending(m, F), keyframe (first frame), step_frame(m, F)
//!                                    [the driver copies sim pos/angle into the header and relinks]
//!   barrier(F, awake)                drain every mover's sim events in slot order (18 §3.4)
//! tail(m) for m in awake             the effect cursor moves to 4n + 4
//! ```
//!
//! The sleep rule (unobserved non-pets are skipped) is the driver's; everything here works on the `awake` list
//! it hands over. The AI and the other docs' event consumers plug in as [`AiDriver`] and [`MoveConsumers`];
//! another `WorldHooks` can be chained behind with [`MovementHooks::with_inner`] (every hook is forwarded to it
//! after the movement code ran).

use super::{FrameLookup, intents, keyframe};
use crate::ai::{AiCmd, AiCx, AiDriver, AiMsg, HelperCall, PetWorld, ScanView, StubPets};
use crate::mover::Mover;
use crate::world::Worlds;
use crate::world::layer::OnDieEntry;
use crate::world::object::{CtrlKey, MoverKey, ObjectStore, RespawnOrigin};
use crate::world::phases::{FrameCtx, WorldHooks};
use crate::world::visibility::Phase;
use flyff_common::{LayerId, ObjId, SimFrame, WorldId};
use flyff_data::types::MenuId;
use flyff_data::worlds::RegionDef;
use flyff_sim::EventMode;
use flyff_sim::step::{MoverLookup, NoRules, SimEnv, SimEvent, SimRole, SimRules, step_frame};
use flyff_sim::types::defs::OT_MOVER;

/// The consumers of sim events owned by other docs (17 combat and skills, 19 items, 21 social). Every method
/// defaults to nothing; they run at the barrier, so their effects start at the next frame and everything they
/// emit is stamped accordingly (31 §3.4).
pub trait MoveConsumers: Send {
    /// A player reached object `target` (a ground item, a ctrl): 19's `item::on_player_arrival` (pickup;
    /// 18 §4.5). Not called for a position arrival.
    fn player_arrived_at_obj(&mut self, _ctx: &mut FrameCtx<'_>, _player: ObjId, _target: ObjId) {}

    /// A player with the pending object action `OBJACT_USESKILL` reached `target`: 17's
    /// `skill::on_dest_obj_arrival` (18 §4.5, §5.9). The pipeline clears the action and stops the player
    /// afterwards.
    fn player_skill_arrival(&mut self, _ctx: &mut FrameCtx<'_>, _player: ObjId, _target: ObjId) {}

    /// Every sim event of every awake mover, after the movement code reacted to it (hits, deaths, jumps,
    /// attack ends, ...): docs 15/16/17/21.
    fn sim_event(
        &mut self,
        _ctx: &mut FrameCtx<'_>,
        _mover: ObjId,
        _frame: SimFrame,
        _ev: &SimEvent,
    ) {
    }

    /// The end of the barrier of frame `frame`, after every event was dispatched and before the mailed AI
    /// messages are delivered: 17's projectile phase steps the live projectiles of the layer here (they home on
    /// the end-of-frame positions).
    fn frame_end(&mut self, _ctx: &mut FrameCtx<'_>, _frame: SimFrame) {}

    /// The tick tail of one awake mover, before the chained hooks' tails (17: the invulnerability counter runs
    /// down, an NPC sweeps its claims).
    fn tail(&mut self, _ctx: &mut FrameCtx<'_>, _mover: MoverKey) {}
}

/// The consumers that do nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoConsumers;

impl MoveConsumers for NoConsumers {}

/// The movement pipeline (see the module docs).
pub struct MovementHooks {
    pub(super) ai: Option<Box<dyn AiDriver>>,
    pub(super) consumers: Box<dyn MoveConsumers>,
    pub(super) rules: Box<dyn SimRules + Send>,
    inner: Option<Box<dyn WorldHooks>>,
    /// The start-of-frame snapshot (also the start-of-tick one while the AI runs).
    pub(super) lookup: FrameLookup,
    /// The after-step snapshot of the current barrier, built on first use.
    pub(super) post: Option<FrameLookup>,
    /// The players of the layer as the target scans see them (captured at the head of the AI pass).
    pub(super) scan: ScanView,
    /// Effects on other movers that the AI handlers queued (18 §8.4); applied right after the handler returns.
    pub(super) cmds: Vec<AiCmd>,
    /// What the eat-pet AI needs from the item system (18 §12): loot rights, the pickup, the dismissal.
    pets: Box<dyn PetWorld>,
}

impl Default for MovementHooks {
    fn default() -> Self {
        MovementHooks::new()
    }
}

impl std::fmt::Debug for MovementHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MovementHooks")
            .field("ai", &self.ai.is_some())
            .field("inner", &self.inner.is_some())
            .finish_non_exhaustive()
    }
}

impl MovementHooks {
    /// The pipeline without AI and without consumers.
    pub fn new() -> MovementHooks {
        MovementHooks {
            ai: None,
            consumers: Box::new(NoConsumers),
            rules: Box::new(NoRules),
            inner: None,
            lookup: FrameLookup::default(),
            post: None,
            scan: ScanView::new(),
            cmds: Vec::new(),
            pets: Box::new(StubPets::new()),
        }
    }

    /// Installs the item system's side of the eat-pet AI (`item::pets::eat::EatPetWorld`, WP-ITEM-18). Without
    /// it the pets use [`StubPets`]: they follow, and they fetch and drop every item the stub allows.
    pub fn with_pets(mut self, pets: Box<dyn PetWorld>) -> MovementHooks {
        self.pets = pets;
        self
    }

    /// Installs the AI driver (WP-AI-06 onward; tests use [`crate::ai::WanderAi`]).
    pub fn with_ai(mut self, ai: Box<dyn AiDriver>) -> MovementHooks {
        self.ai = Some(ai);
        self
    }

    /// Installs the consumers of the sim events of docs 15-21.
    pub fn with_consumers(mut self, c: Box<dyn MoveConsumers>) -> MovementHooks {
        self.consumers = c;
        self
    }

    /// Installs the game-rule predicates the sim consults (doc 17's `SimRules`).
    pub fn with_rules(mut self, r: Box<dyn SimRules + Send>) -> MovementHooks {
        self.rules = r;
        self
    }

    /// Chains another set of hooks behind the movement pipeline: it receives every hook after the movement
    /// code ran (tails, `_OnDie`, `_replace`, layer GC, ...).
    pub fn with_inner(mut self, inner: Box<dyn WorldHooks>) -> MovementHooks {
        self.inner = Some(inner);
        self
    }

    /// The installed AI driver, for tests that read its counters.
    pub fn ai(&self) -> Option<&dyn AiDriver> {
        self.ai.as_deref()
    }

    /// Installs the monster brains of doc 18 ([`crate::ai::MonsterAiDriver`]).
    pub fn with_monster_ai(self) -> MovementHooks {
        self.with_ai(Box::new(crate::ai::MonsterAiDriver))
    }

    /// Builds the AI context for `frame` (the after-step snapshot when it exists, the start-of-frame one
    /// otherwise) and runs `f` with the installed driver (`None` without one), the mover `key` and the
    /// context. `None` when the mover is gone.
    pub(crate) fn with_ai_cx<R>(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        frame: SimFrame,
        key: MoverKey,
        f: impl FnOnce(Option<&mut (dyn AiDriver + 'static)>, &mut Mover, &mut AiCx<'_>) -> R,
    ) -> Option<R> {
        let m = ctx.store.movers.get_mut(key)?;
        let instance = self
            .inner
            .as_ref()
            .is_some_and(|i| i.is_instance_world(ctx.world));
        let lookup: &dyn MoverLookup = self.post.as_ref().unwrap_or(&self.lookup);
        let mut cx = AiCx {
            tick: ctx.tick,
            now: ctx.now,
            frame,
            world: ctx.world,
            layer: ctx.layer,
            instance,
            rng: &mut *ctx.rng,
            rules: &mut *self.rules,
            shared: &ctx.movement.shared,
            collision: &ctx.statics.collision,
            dynamic: ctx.dynamic,
            lookup,
            grid: ctx.grid,
            scan: &self.scan,
            sink: &mut *ctx.sink,
            cmds: &mut self.cmds,
        };
        Some(f(self.ai.as_deref_mut(), m, &mut cx))
    }

    /// Delivers `msg` to mover `to` as `PostAIMsg` would (`Arrival` queued, the rest synchronous) in the frame
    /// `frame`. A mover that is gone or flagged for deletion does not hear it.
    pub(super) fn deliver_ai(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        frame: SimFrame,
        to: ObjId,
        msg: AiMsg,
    ) {
        let Some(key) = ctx.store.mover_key(to) else {
            return;
        };
        if ctx.store.movers.get(key).is_none_or(|m| m.core.deleted) {
            return;
        }
        self.with_ai_cx(ctx, frame, key, |ai, m, cx| {
            if let Some(ai) = ai {
                ai.message(m, msg, cx);
            }
        });
    }

    /// Applies the commands the AI handlers queued, in order. Delivered messages may queue more commands; the
    /// chain is cut after a few rounds (a loop between two AIs would be a bug, never V15 logic).
    pub(super) fn drain_cmds(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame) {
        for _ in 0..8 {
            if self.cmds.is_empty() {
                return;
            }
            let cmds = std::mem::take(&mut self.cmds);
            for c in cmds {
                match c {
                    AiCmd::Send { to, msg } => self.deliver_ai(ctx, frame, to, msg),
                    AiCmd::Delete(id) => ctx.delete(id),
                    AiCmd::Spawn(s) => ctx.spawn(s),
                    AiCmd::RemoveEnemies { who, enemies } => {
                        // `RemoveEnemy(who)` on every enemy still there, from the highest id down. Without the
                        // combat environment (no `combat::tick::prepare` yet) the list was cleared and nothing
                        // else is known to the enemies.
                        let _ = crate::combat::with_action_phase(ctx, frame, |cx, _| {
                            for enemy in enemies.into_iter().rev() {
                                if cx.mover(enemy).is_some() {
                                    crate::combat::remove_enemy(
                                        &mut cx.arb.w,
                                        &mut cx.arb.fx,
                                        enemy,
                                        who,
                                    );
                                }
                            }
                        });
                    }
                    AiCmd::CallHelpers(c) => self.call_helpers(ctx, frame, &c),
                    // ---- boss effects (doc 18 §8.4, task boss-ai-b) ----
                    AiCmd::DamageAround {
                        attacker,
                        center,
                        flags,
                        item,
                        radius,
                    } => {
                        // no combat environment yet: dropped, like `RemoveEnemies`
                        let _ = crate::combat::with_action_phase(ctx, frame, |cx, _| {
                            crate::ai::boss::fx::damage_around_at(
                                cx, attacker, center, flags, item, radius,
                            );
                        });
                    }
                    AiCmd::Replace {
                        player,
                        world,
                        layer,
                        pos,
                    } => ctx.replace(
                        player,
                        world,
                        layer,
                        pos,
                        crate::world::replace::ReplaceKind::Normal,
                    ),
                    AiCmd::Boss(fx) => ctx
                        .movement
                        .combat
                        .queues
                        .skill_jobs
                        .push(crate::skill::cast::SkillJob::Boss(fx)),
                }
            }
        }
        if !self.cmds.is_empty() {
            tracing::warn!(left = self.cmds.len(), "AI command chain cut");
            self.cmds.clear();
        }
    }

    /// `CAIMonster::CallHelper` (18 §9.9): picks the movers of the legacy dynamic-grid window around the caller
    /// and sends them `SetState{RAGE, target}`; a slot with the party flag sends each helper a random member of
    /// the target's party (the players of the party within 20 m of the caller) instead.
    pub(super) fn call_helpers(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, c: &HelperCall) {
        use crate::world::grid::LinkSet;
        const MAX_ARRAY: usize = 40;
        let mpu = ctx.grid.geom().mpu as i32;
        let n_range = c.n_range / mpu.max(1);
        // the helpers of every slot, in window order
        let mut found: Vec<Vec<ObjId>> = vec![Vec::new(); c.slots.len()];
        let mut full = false;
        ctx.grid
            .for_each_legacy(c.pos, n_range, LinkSet::Others, |id| {
                if full || id == c.caller {
                    return;
                }
                let Some(m) = ctx.store.mover(id) else { return };
                for (i, slot) in c.slots.iter().enumerate() {
                    let same = c.help_who == 2 && m.prop_id() == c.caller_mover;
                    let count = found[i].len();
                    if (c.help_who == 1 || same) && (slot.num == 0 || count < slot.num as usize) {
                        found[i].push(id);
                        if found[i].len() >= MAX_ARRAY {
                            full = true;
                            return;
                        }
                    }
                }
            });
        // the target's party, when some slot wants it and the target has one
        let wants_party = c.slots.iter().any(|s| s.party);
        let mut members: Vec<ObjId> = Vec::new();
        let has_party = wants_party && c.target.is_some() && c.target_party != 0;
        if has_party {
            ctx.grid
                .for_each_legacy(c.pos, 20 / mpu.max(1), LinkSet::Players, |id| {
                    if members.len() < MAX_ARRAY
                        && self.scan.get(id).is_some_and(|p| p.party == c.target_party)
                    {
                        members.push(id);
                    }
                });
        }
        for helpers in found {
            if has_party {
                if members.is_empty() {
                    continue;
                }
                for h in helpers {
                    // `rand() % nCountPlayer` (PL-013: the world stream)
                    let pick = ctx.rng.next_u32() as usize % members.len();
                    let state = AiMsg::SetState {
                        state: crate::ai::MonsterState::Rage as u32,
                        param: members.get(pick).copied(),
                    };
                    self.deliver_ai(ctx, frame, h, state);
                }
            } else {
                for h in helpers {
                    let state = AiMsg::SetState {
                        state: crate::ai::MonsterState::Rage as u32,
                        param: c.target,
                    };
                    self.deliver_ai(ctx, frame, h, state);
                }
            }
        }
    }

    /// Delivers the mailed AI messages (`ai::mail`) whose target is in the layer being processed; mail for other
    /// layers waits for their turn, mail for movers that no longer exist is dropped.
    ///
    /// At a barrier `awake` is the layer's awake list: the after-step snapshot the handlers read is built
    /// from it first (the start-of-tick snapshot serves the AI pass).
    pub(super) fn flush_mail(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        frame: SimFrame,
        awake: Option<&[MoverKey]>,
    ) {
        if ctx.movement.ai_mail.is_empty() {
            return;
        }
        if let Some(awake) = awake
            && self.post.is_none()
        {
            self.post = Some(FrameLookup::capture(
                ctx.store,
                awake,
                &ctx.movement.shared.flags,
            ));
        }
        let mail = std::mem::take(&mut ctx.movement.ai_mail);
        let mut keep = Vec::new();
        for (to, msg) in mail {
            match ctx.store.mover(to) {
                None => {}
                Some(m) if m.core.world == ctx.world && m.core.layer == ctx.layer => {
                    self.deliver_ai(ctx, frame, to, msg);
                    self.drain_cmds(ctx, frame);
                }
                Some(_) => keep.push((to, msg)),
            }
        }
        // mail the handlers posted meanwhile goes behind the kept ones
        let posted = std::mem::replace(&mut ctx.movement.ai_mail, keep);
        ctx.movement.ai_mail.extend(posted);
    }
}

/// First use of a mover by the pipeline: the model bounds of a prop-based mover (`IsRangeObj` ranges and the
/// collision radius read them; 31 §11.5).
fn init_mover(m: &mut Mover, shared: &super::MoveShared) {
    m.sync.inited = true;
    if m.is_player() {
        return;
    }
    if let Some(bounds) = &shared.bounds
        && let Some(file) = shared.data.models.model_file(OT_MOVER, m.core.index)
        && let Some(b) = bounds.get(&format!("model/{}", file.to_ascii_lowercase()))
    {
        m.sim.params.bounds = b;
    }
}

impl WorldHooks for MovementHooks {
    fn on_phase(&mut self, world: WorldId, layer: LayerId, phase: Phase) {
        if let Some(i) = self.inner.as_mut() {
            i.on_phase(world, layer, phase);
        }
    }

    fn init_ai(&mut self, store: &mut ObjectStore, id: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.init_ai(store, id);
        }
    }

    fn on_npc_added(&mut self, id: ObjId, world: WorldId, layer: LayerId, menus: &[MenuId]) {
        if let Some(i) = self.inner.as_mut() {
            i.on_npc_added(id, world, layer, menus);
        }
    }

    fn ai_begin(&mut self, ctx: &mut FrameCtx<'_>, awake: &[MoverKey]) {
        ctx.movement.stats.awake = awake.len() as u64;
        if self.ai.is_some() {
            self.lookup = FrameLookup::capture(ctx.store, awake, &ctx.movement.shared.flags);
            self.scan
                .capture(ctx.store, awake, &ctx.movement.shared.data);
            self.post = None;
            // what phase 1 and the previous tick's tail mailed to this layer is heard first (frame 4n)
            let frame = ctx.tick.first_frame();
            self.flush_mail(ctx, frame, None);
        }
        if let Some(i) = self.inner.as_mut() {
            i.ai_begin(ctx, awake);
        }
    }

    fn ai(&mut self, ctx: &mut FrameCtx<'_>, key: MoverKey) {
        let tick = ctx.tick;
        if let Some(m) = ctx.store.movers.get_mut(key) {
            // 18 §5.2 rule 4: whatever was queued before the mover went to sleep is stale
            if !m.is_player() && m.sync.pre_tick.saturating_add(1) < tick.0 {
                m.sync.pending.clear();
            }
            m.sync.pre_tick = tick.0;
            m.sync.next_pre_frame = tick.first_frame().0;
            let first = !m.sync.inited;
            if first {
                init_mover(m, &ctx.movement.shared);
            }
            if crate::ai::pet::is_pet(m) {
                // 18 §12: the eat pets have their own pass (it needs the ground items and the owner's inventory)
                self.post = None;
                let mut pets =
                    std::mem::replace(&mut self.pets, Box::new(crate::ai::pet::Detached));
                crate::ai::pet::pass(self, ctx, key, &mut *pets);
                self.pets = pets;
                self.drain_cmds(ctx, tick.first_frame());
            } else if !m.is_player() && self.ai.is_some() {
                let frame = tick.first_frame();
                self.post = None;
                self.with_ai_cx(ctx, frame, key, |ai, m, cx| {
                    if let Some(ai) = ai {
                        if first {
                            ai.init(m, cx);
                        }
                        ai.process(m, cx);
                    }
                });
                self.drain_cmds(ctx, frame);
            }
        }
        if let Some(i) = self.inner.as_mut() {
            i.ai(ctx, key);
        }
    }

    fn snapshot(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, awake: &[MoverKey]) {
        self.lookup = FrameLookup::capture(ctx.store, awake, &ctx.movement.shared.flags);
        self.post = None;
        if let Some(i) = self.inner.as_mut() {
            i.snapshot(ctx, frame, awake);
        }
    }

    fn step(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, key: MoverKey) {
        let tick = ctx.tick;
        if let Some(m) = ctx.store.movers.get_mut(key) {
            let mv = &mut *ctx.movement;
            let shared = &*mv.shared;
            // 18 §3.3 step 6a/6b: the stamps of this mover's queued intents and its keyframe
            intents::capture_pending(m, frame, &mut *ctx.sink, &mut mv.stats);
            if frame.0.is_multiple_of(flyff_common::time::FRAMES_PER_TICK) {
                keyframe::maybe_emit(m, tick, frame, shared, &mut *ctx.sink, &mut mv.stats);
            }
            // 6c: the shared sim, one 1/60 s frame including collision
            let env = SimEnv {
                frame,
                role: SimRole::Server,
                flags: shared.flags,
                world: &ctx.statics.collision,
                dynamic: ctx.dynamic,
                motions: &shared.motions,
                lookup: &self.lookup,
                mode: EventMode::Normal,
            };
            step_frame(
                &mut m.sim,
                &env,
                &mut *self.rules,
                &mut *ctx.rng,
                &mut m.sync.events,
            );
            mv.stats.sim_frames += 1;
            // a hostile state can drive the sim to infinity (a client owns its own state, D1, but the world
            // must stay finite): fall back to the last good position the driver copied into the header
            if !(m.sim.pos.is_finite()
                && m.sim.act.delta.is_finite()
                && m.sim.act.delta_e.is_finite())
            {
                m.sim.pos = m.core.pos;
                m.sim.act.delta = flyff_common::math::Vec3::ZERO;
                m.sim.act.delta_e = flyff_common::math::Vec3::ZERO;
                mv.stats.sanitised += 1;
            }
        }
        if let Some(i) = self.inner.as_mut() {
            i.step(ctx, frame, key);
        }
    }

    fn barrier(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame, awake: &[MoverKey]) {
        self.barrier_events(ctx, frame, awake);
        if let Some(i) = self.inner.as_mut() {
            i.barrier(ctx, frame, awake);
        }
    }

    fn tail(&mut self, ctx: &mut FrameCtx<'_>, key: MoverKey) {
        let tick = ctx.tick;
        if let Some(m) = ctx.store.movers.get_mut(key) {
            m.sync.pre_tick = tick.0;
            m.sync.next_pre_frame = tick.frames_completed_after().0;
        }
        // 18 §6: the worn ride and the flight state of the sim copy agree at the end of every tick
        super::flying::tail(ctx, key);
        self.consumers.tail(ctx, key);
        if let Some(i) = self.inner.as_mut() {
            i.tail(ctx, key);
        }
    }

    fn process_ctrl(&mut self, ctx: &mut FrameCtx<'_>, ctrl: CtrlKey) {
        if let Some(i) = self.inner.as_mut() {
            i.process_ctrl(ctx, ctrl);
        }
    }

    fn layer_process(&mut self, ctx: &mut FrameCtx<'_>) {
        if let Some(i) = self.inner.as_mut() {
            i.layer_process(ctx);
        }
    }

    fn on_die(
        &mut self,
        worlds: &mut Worlds,
        world: WorldId,
        layer: LayerId,
        entries: Vec<OnDieEntry>,
    ) {
        if let Some(i) = self.inner.as_mut() {
            i.on_die(worlds, world, layer, entries);
        }
    }

    fn replace(&mut self, worlds: &mut Worlds, world: WorldId, layer: LayerId) {
        if let Some(i) = self.inner.as_mut() {
            i.replace(worlds, world, layer);
        }
    }

    fn replace_leaving(
        &mut self,
        store: &mut ObjectStore,
        player: ObjId,
        from: (WorldId, LayerId),
        to: (WorldId, LayerId),
    ) {
        if let Some(i) = self.inner.as_mut() {
            i.replace_leaving(store, player, from, to);
        }
    }

    fn world_change_entered(&mut self, ctx: &mut FrameCtx<'_>, player: MoverKey) {
        if let Some(i) = self.inner.as_mut() {
            i.world_change_entered(ctx, player);
        }
    }

    fn region_class_changed(&mut self, store: &mut ObjectStore, player: ObjId, old: u32, new: u32) {
        if let Some(i) = self.inner.as_mut() {
            i.region_class_changed(store, player, old, new);
        }
    }

    fn is_instance_world(&self, world: WorldId) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|i| i.is_instance_world(world))
    }

    fn instance_enter(&mut self, store: &mut ObjectStore, player: ObjId, region: &RegionDef) {
        if let Some(i) = self.inner.as_mut() {
            i.instance_enter(store, player, region);
        }
    }

    fn on_npc_removed(&mut self, id: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.on_npc_removed(id);
        }
    }

    fn on_destroyed(
        &mut self,
        world: WorldId,
        layer: LayerId,
        origin: RespawnOrigin,
        aggressive: bool,
    ) {
        if let Some(i) = self.inner.as_mut() {
            i.on_destroyed(world, layer, origin, aggressive);
        }
    }

    fn layer_gc(&mut self, worlds: &mut Worlds, world: WorldId) {
        if let Some(i) = self.inner.as_mut() {
            i.layer_gc(worlds, world);
        }
    }
}
