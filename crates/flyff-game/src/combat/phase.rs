//! Combat in the world phase (Plan/17 §3.7, §4.1; WP-COMBAT-05, -06, -09): [`CombatConsumers`] is the
//! [`MoveConsumers`] of the movement pipeline that reacts to the sim events of the barrier of a frame.
//!
//! | event | reaction |
//! |---|---|
//! | `AttrHit` | the hit-frame handlers ([`super::action::on_attr_hit`]): the damage arbiter, the projectile spawns |
//! | `RepeatShot` | the skill seam ([`super::skillhit::SkillHitHooks::on_repeat_shot`]) |
//! | `AttackStarted` (non-player) | the hit roll and the attack notification of a monster ([`super::action::on_npc_attack_started`]) |
//! | `MeleeContinue` (boss models) | the scripted continuation of a boss swing, the ClockWorks laser ([`crate::ai::boss::hits::on_attack_melee_continue`]) |
//! | `DrainMagicQueue` | the next queued wand or bow attack of a player ([`super::action::drain_magic_queue`]) |
//! | `JJim` | the kill-steal claim `target.id_targeter = attacker` ([`super::target::set_jjim`]) |
//! | `RemoveInvisible` | the attacker loses its invisibility (the buff engine's seam) |
//!
//! At the end of every barrier [`MoveConsumers::frame_end`] steps the projectiles of the layer
//! ([`super::projectile::step_layer`]), and at the tick tail of every awake mover ([`MoveConsumers::tail`]) the
//! invulnerability counter runs down, the pink window of a PK hit ends and an NPC sweeps its claims ([`super::target::process_target`]).
//!
//! The consumer chains another one behind it ([`CombatConsumers::with_inner`]): the pipeline has a single slot.

use super::action::{self, HitEvent};
use super::context::with_action_phase;
use super::projectile;
use super::target;
use crate::movement::MoveConsumers;
use crate::world::object::MoverKey;
use crate::world::phases::FrameCtx;
use flyff_common::{ObjId, SimFrame};
use flyff_sim::step::SimEvent;

/// The combat reactions of the world phase (see the module documentation).
#[derive(Default)]
pub struct CombatConsumers {
    inner: Option<Box<dyn MoveConsumers>>,
}

impl std::fmt::Debug for CombatConsumers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CombatConsumers")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl CombatConsumers {
    /// The combat consumer alone.
    pub fn new() -> CombatConsumers {
        CombatConsumers { inner: None }
    }

    /// Chains `inner` behind it: it receives every call after the combat code ran.
    pub fn with_inner(mut self, inner: Box<dyn MoveConsumers>) -> CombatConsumers {
        self.inner = Some(inner);
        self
    }
}

impl MoveConsumers for CombatConsumers {
    fn player_arrived_at_obj(&mut self, ctx: &mut FrameCtx<'_>, player: ObjId, target: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.player_arrived_at_obj(ctx, player, target);
        }
    }

    fn player_skill_arrival(&mut self, ctx: &mut FrameCtx<'_>, player: ObjId, target: ObjId) {
        if let Some(i) = self.inner.as_mut() {
            i.player_skill_arrival(ctx, player, target);
        }
    }

    fn sim_event(&mut self, ctx: &mut FrameCtx<'_>, mover: ObjId, frame: SimFrame, ev: &SimEvent) {
        on_sim_event(ctx, mover, frame, ev);
        if let Some(i) = self.inner.as_mut() {
            i.sim_event(ctx, mover, frame, ev);
        }
    }

    fn frame_end(&mut self, ctx: &mut FrameCtx<'_>, frame: SimFrame) {
        let (world, layer) = (ctx.world, ctx.layer);
        if !ctx.movement.combat.queues.projectiles.is_empty() {
            with_action_phase(ctx, SimFrame(frame.0 + 1), |cx, skill| {
                projectile::step_layer(cx, skill, world, layer, frame);
            });
        }
        if let Some(i) = self.inner.as_mut() {
            i.frame_end(ctx, frame);
        }
    }

    fn tail(&mut self, ctx: &mut FrameCtx<'_>, mover: MoverKey) {
        let mut sweep = None;
        let now = ctx.now;
        if let Some(m) = ctx.store.movers.get_mut(mover) {
            // `CUser::Process`: the pink window of a PK hit ends (`PkRelation{PINK, 0}` to the viewers)
            if m.is_player() && super::pvp::expire_pink(m, now) {
                let mut out = crate::stats::out::Out::new();
                out.near(super::pvp::pk::pink_relation(false));
                out.flush_sink(&mut *ctx.sink, m);
            }
            // `if (m_nDead > 0) --m_nDead` (Process): the invulnerability after a revival runs down
            if m.combat.dead_ticks > 0 {
                m.combat.dead_ticks -= 1;
            }
            // `ProcessActionPoint` of a live player (WP-SKILL-09)
            if m.is_player() && crate::combat::is_live(m) {
                crate::skill::queue::process_action_point(m, ctx.now);
            }
            // `ProcessTarget` every 128 ticks of an NPC
            if !m.is_player() && m.mv.count & 127 == 0 {
                sweep = Some(m.core.id);
            }
        }
        if let Some(id) = sweep {
            target::process_target(ctx.store, id);
        }
        // the damage meter of a tracked monster (17 §10, WP-COMBAT-14)
        super::dps::tick_mover(ctx, mover);
        if let Some(i) = self.inner.as_mut() {
            i.tail(ctx, mover);
        }
    }
}

/// The combat reaction to one sim event of `mover` at the barrier of `frame` (effects start at `frame + 1`).
pub fn on_sim_event(ctx: &mut FrameCtx<'_>, mover: ObjId, frame: SimFrame, ev: &SimEvent) {
    let after = SimFrame(frame.0 + 1);
    match *ev {
        SimEvent::AttrHit {
            kind,
            state,
            target,
            hit_index,
            frame: f,
        } => {
            let hit = HitEvent {
                kind,
                state,
                target,
                hit_index,
                frame: f,
            };
            with_action_phase(ctx, after, |cx, skill| {
                action::on_attr_hit(cx, skill, mover, hit);
            });
        }
        SimEvent::RepeatShot { kind, frame: f } => {
            with_action_phase(ctx, after, |cx, skill| {
                action::on_repeat_shot(cx, skill, mover, kind, f);
            });
        }
        SimEvent::AttackStarted { target, msg } => {
            let is_npc = ctx.store.mover(mover).is_some_and(|m| !m.is_player());
            if is_npc {
                with_action_phase(ctx, after, |cx, _| {
                    action::on_npc_attack_started(cx, mover, msg, target);
                });
            }
        }
        // WP-AI-18: a boss's scripted swing goes on while `MVRF_HITCONT` is set (the ClockWorks laser); every
        // melee monster raises this event each frame after its hit, so the cheap prop filter comes first
        SimEvent::MeleeContinue {
            state,
            count,
            hit_count,
        } => {
            let wanted = ctx.store.mover(mover).is_some_and(|m| {
                !m.is_player() && crate::ai::boss::hits::wants_continue(m.prop_id())
            });
            if wanted {
                with_action_phase(ctx, after, |cx, _| {
                    crate::ai::boss::hits::on_attack_melee_continue(
                        cx, mover, state, count, hit_count,
                    );
                });
            }
        }
        SimEvent::DrainMagicQueue => {
            let is_player = ctx.store.mover(mover).is_some_and(|m| m.is_player());
            if is_player {
                with_action_phase(ctx, after, |cx, _| action::drain_magic_queue(cx, mover));
            }
        }
        // WP-SKILL-05 / -09: the end of a skill state (the act is forgotten, a running action-slot chain asks for
        // its next skill), a cancelled cast, and the arrival of a queued skill's approach
        SimEvent::MeleeSkillEnded | SimEvent::MagicSkillEnded => {
            with_action_phase(ctx, after, |cx, _| {
                crate::skill::cast::on_skill_end(cx, mover)
            });
        }
        SimEvent::CastCancelled => {
            with_action_phase(ctx, after, |cx, _| {
                crate::skill::cast::on_cast_cancelled(cx, mover);
            });
        }
        SimEvent::ArrivedAtObj { target, cmd }
            if cmd == flyff_sim::types::OBJACT_USESKILL
                && ctx.store.mover(mover).is_some_and(|m| m.is_player()) =>
        {
            with_action_phase(ctx, after, |cx, _| {
                crate::skill::queue::on_arrival(cx, mover, target);
            });
        }
        SimEvent::JJim { target: t } => {
            let data = std::sync::Arc::clone(&ctx.movement.shared.data);
            target::set_jjim(ctx.store, &data, mover, t);
        }
        SimEvent::RemoveInvisible => {
            with_action_phase(ctx, after, |cx, _| {
                cx.arb
                    .hooks
                    .remove_invisible(&mut cx.arb.w, &mut cx.arb.fx, mover);
            });
        }
        _ => {}
    }
}
