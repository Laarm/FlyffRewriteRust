//! The working context of the action layer (Plan/17 §3; WP-COMBAT-05): [`ActionCx`] bundles the damage pipeline's
//! [`ArbiterCx`] with the sim context, the projectile tables and the switches, and [`with_action`] /
//! [`with_action_phase`] build it in the two places combat code runs:
//!
//! * a **client handler** (phase 1, [`with_action`]): the services and the [`GameCtx`] exist, the effects are
//!   delivered through the outbox, and the deaths the call caused are processed before it returns
//!   ([`super::death::process_pending`]);
//! * a **hit frame at the barrier** of the movement phase ([`with_action_phase`]): only the object store, the
//!   world's random stream and the layer's message sink exist; the effects go through the sink and the deaths
//!   wait for the service-context step after the world phase.
//!
//! [`StartRules`] is the [`SimRules`] the sim consults while an attack starts: the sim asks `is_steal` (answered
//! from a pre-evaluated, message-free decision) and `attack_result` (answered "hit" and *recorded*; the real roll
//! is drawn right after the call returns, from the same random stream, which is the same draw position as the
//! original: the roll is the last statement of `ProcessActMsg`'s ATKn handler).

use super::arbiter::steal_check;
use super::effects::{Arena, PhaseDelivery, SimCtx};
use super::env::ArbiterCx;
use super::skillhit::SkillHitHooks;
use super::world::{ActionConfig, CombatQueues, CombatWorld};
use crate::ctx::{GameCtx, Services};
use crate::movement::FrameLookup;
use crate::mover::Mover;
use crate::world::grid::Grid;
use crate::world::phases::FrameCtx;
use flyff_common::{ObjId, SimFrame};
use flyff_sim::SimMover;
use flyff_sim::step::{Cx, SimRules};
use std::cell::Cell;

/// What one combat call works with (see the module documentation).
pub struct ActionCx<'a> {
    /// The damage pipeline's context: the arena (store, world RNG, clock), the collected effects, the rule
    /// environment and the seams.
    pub arb: ArbiterCx<'a>,
    /// The sim context: the frame the effects of this call take, the shared tables, the collision world.
    pub sc: SimCtx<'a>,
    /// The projectile table and the death queue.
    pub q: &'a mut CombatQueues,
    /// The action-layer switches.
    pub act: ActionConfig,
    /// The link-map grid of the layer (area attacks walk its player and monster lists).
    pub grid: &'a Grid,
}

impl ActionCx<'_> {
    /// The mover `id` (not flagged for deletion).
    pub fn mover(&self, id: ObjId) -> Option<&Mover> {
        self.arb.w.store.mover(id).filter(|m| !m.core.deleted)
    }

    /// The mover `id` for writing (not flagged for deletion).
    pub fn mover_mut(&mut self, id: ObjId) -> Option<&mut Mover> {
        self.arb.w.store.mover_mut(id).filter(|m| !m.core.deleted)
    }

    /// Runs `f` on the sim of mover `id` with a sim context of [`ActionCx::sc`]: the lookup contains `id` and
    /// every object of `extra` (targets the sim must see), the rules are `rules`, the random stream is the
    /// world's, the events go to the mover's buffer. `None` when the mover does not exist.
    pub fn sim_call<R>(
        &mut self,
        id: ObjId,
        extra: &[ObjId],
        rules: &mut dyn SimRules,
        f: impl FnOnce(&mut SimMover, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let mut ids = Vec::with_capacity(extra.len() + 1);
        ids.push(id);
        ids.extend_from_slice(extra);
        let lookup = FrameLookup::for_objects(self.arb.w.store, &ids, &self.sc.shared.flags);
        let env = self.sc.env(&lookup);
        let m = self.arb.w.store.mover_mut(id)?;
        let Mover { sim, sync, .. } = m;
        let mut cx = Cx::new(&env, rules, &mut *self.arb.w.rng, &mut sync.events);
        Some(f(sim, &mut cx))
    }
}

/// The rules of an attack start (see the module documentation).
#[derive(Debug, Default)]
pub struct StartRules {
    steal: bool,
    steal_asked: Cell<bool>,
    roll: Option<(ObjId, ObjId)>,
}

impl StartRules {
    /// Rules for an attack by `attacker` on `target`: kill-steal protection answers per the pre-evaluated
    /// decision.
    pub fn for_attack(cx: &ActionCx<'_>, attacker: ObjId, target: ObjId) -> StartRules {
        let steal = cx.arb.env.steal_protection
            && steal_check(cx.arb.w.store, &cx.arb.env.core.data, attacker, target);
        StartRules {
            steal,
            steal_asked: Cell::new(false),
            roll: None,
        }
    }

    /// Rules without kill-steal protection and without a roll to take (a plain sim call).
    pub fn plain() -> StartRules {
        StartRules::default()
    }

    /// `true` when the sim asked `is_steal` and the answer was "stolen": the caller sends
    /// `TID_GAME_PRIORITYMOB` to the attacker.
    pub fn stole(&self) -> bool {
        self.steal_asked.get()
    }

    /// The `(attacker, target)` of the hit roll the sim asked for, if it did.
    pub fn roll_wanted(&self) -> Option<(ObjId, ObjId)> {
        self.roll
    }
}

impl SimRules for StartRules {
    fn is_steal(&self, _attacker: ObjId, _target: ObjId) -> bool {
        if self.steal {
            self.steal_asked.set(true);
        }
        self.steal
    }

    fn attack_result(&mut self, attacker: ObjId, target: ObjId, _mop: u32) -> bool {
        self.roll = Some((attacker, target));
        true
    }
}

/// Builds an [`ActionCx`] for a client handler or a service-side caller, runs `f`, delivers the effects through
/// the outbox and processes the deaths the call caused. `world_of` names the mover whose world supplies the
/// random stream, the collision data and the layer; `None` when that mover does not exist.
pub fn with_action<R>(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    world_of: ObjId,
    f: impl FnOnce(&mut ActionCx<'_>) -> R,
) -> Option<R> {
    let frame = ctx.clock.frame0;
    let r = with_action_at(s, ctx, world_of, frame, f)?;
    super::death::process_pending(s, ctx);
    crate::skill::cast::drain_jobs(s, ctx);
    Some(r)
}

/// [`with_action`] at an explicit frame and without processing the deaths (the death pipeline's own use).
pub fn with_action_at<R>(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    world_of: ObjId,
    frame: SimFrame,
    f: impl FnOnce(&mut ActionCx<'_>) -> R,
) -> Option<R> {
    let core = ctx.worlds.core(world_of)?;
    let (world, layer) = (core.world, core.layer);
    let statics = ctx.worlds.statics_of(world_of)?.clone();
    let shared = ctx.worlds.movement.shared.clone();
    let env = super::arbiter::damage_env(s, ctx);
    let act = ActionConfig::from_config(&ctx.cfg.rules).with_events(ctx.live);
    let (r, fx) = {
        let GameCtx {
            worlds, rng, clock, ..
        } = &mut *ctx;
        let dynamic = worlds.dynamic_of(world, layer);
        let (store, movement, grid) = worlds.split_for_combat(world, layer)?;
        let CombatWorld { hooks, queues, .. } = &mut movement.combat;
        let w = Arena {
            store,
            rng: rng.world(world),
            now: clock.game_ms,
            tick: clock.tick,
            frame: Some(frame),
        };
        let mut cx = ActionCx {
            arb: ArbiterCx::new(w, &env, &mut **hooks),
            sc: SimCtx {
                frame,
                shared: &shared,
                collision: &statics.collision,
                dynamic: &dynamic,
            },
            q: queues,
            act,
            grid,
        };
        let r = f(&mut cx);
        (r, cx.arb.fx)
    };
    fx.deliver_game_at(ctx, frame);
    Some(r)
}

/// Builds an [`ActionCx`] for the barrier of the movement phase from the pieces of a [`FrameCtx`], runs `f` and
/// delivers the effects through the layer's sink. `frame` is the first frame the effects take (`F + 1` at the
/// barrier of frame `F`). `None` (and `f` is not run) before the first [`super::tick::prepare`].
pub fn with_action_phase<R>(
    ctx: &mut FrameCtx<'_>,
    frame: SimFrame,
    f: impl FnOnce(&mut ActionCx<'_>, &mut dyn SkillHitHooks) -> R,
) -> Option<R> {
    let cenv = ctx.movement.combat.env.clone()?;
    let env = cenv.damage_env();
    let shared = std::sync::Arc::clone(&ctx.movement.shared);
    let FrameCtx {
        store,
        rng,
        movement,
        sink,
        statics,
        tick,
        now,
        grid,
        dynamic,
        ..
    } = ctx;
    let crate::movement::MovementState {
        combat, ai_mail, ..
    } = &mut **movement;
    let CombatWorld {
        hooks,
        skill,
        queues,
        ..
    } = combat;
    let w = Arena {
        store,
        rng: &mut **rng,
        now: *now,
        tick: *tick,
        frame: Some(frame),
    };
    let mut cx = ActionCx {
        arb: ArbiterCx::new(w, &env, &mut **hooks),
        sc: SimCtx {
            frame,
            shared: &shared,
            collision: &statics.collision,
            dynamic,
        },
        q: queues,
        act: cenv.act,
        grid,
    };
    let r = f(&mut cx, &mut **skill);
    let ActionCx { arb, sc, q, .. } = cx;
    let ArbiterCx { w, fx, .. } = arb;
    let Arena { store, rng, .. } = w;
    let (mut spawns, mut deletes) = (Vec::new(), Vec::new());
    fx.deliver_phase(PhaseDelivery {
        store,
        rng,
        sc: &sc,
        sink: &mut **sink,
        mail: ai_mail,
        deaths: &mut q.deaths,
        spawns: &mut spawns,
        deletes: &mut deletes,
        skill_jobs: &mut q.skill_jobs,
    });
    // the object-creating step runs right after the hook (doc 15 `_add` / `_delete` queues)
    for s in spawns {
        ctx.spawn(s);
    }
    for id in deletes {
        ctx.delete(id);
    }
    Some(r)
}
