//! The world access and the collected effects of one damage call (Plan/17 §5; WP-COMBAT-04).
//!
//! The damage pipeline runs in two kinds of context: client handlers and GM commands hold the services and the
//! [`GameCtx`], the hit events of the movement phase hold only the object store, the world's random stream and
//! a message sink. The arbiter therefore works on an [`Arena`] (the store, the random stream, the clock) and
//! **collects** what it wants to happen outside the movers into an [`Effects`] value:
//!
//! * [`Emit`]: messages, in order, with their audience (the `Damage` broadcast, `SetPointParam` after AUTOHP, ...),
//! * AI mail for the movement pipeline (`AIMSG_DAMAGE`, `AIMSG_DSTDIE`),
//! * [`SimFx`]: the calls into the shared sim that need its environment (`OBJMSG_STAND`, the damage motion),
//! * the movers that reached 0 HP ([`KillCtx`]), for the service-context death pipeline.
//!
//! The caller delivers the value when the call returns: [`Effects::deliver_game`] for a handler context; the
//! movement phase delivers the messages through its sink and applies the [`SimFx`] with its own sim context
//! ([`Effects::deliver_phase`]).

use super::hooks::KillCtx;
use crate::ai::AiMsg;
use crate::ctx::GameCtx;
use crate::movement::{FrameLookup, MoveShared};
use crate::mover::Mover;
use crate::stats::out::{Aud, Out};
use crate::world::object::ObjectStore;
use crate::world::visibility::SpawnSink;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, ObjId, SimFrame, Tick};
use flyff_proto::pb;
use flyff_sim::collision::dynamic::DynamicColliders;
use flyff_sim::collision::world::CollisionWorld;
use flyff_sim::step::{Cx, EventMode, NoRules, SimEnv, SimRole};
use flyff_sim::types::{OBJMSG_STAND, OBJMSG_STOP, OBJMSG_STUN, OBJSTA_ATK_ALL, OBJSTA_MOVE_ALL};
use flyff_sim::{ActMsg, SimMover};

/// The mutable world of one damage call: the objects, the world's random stream and the clock.
pub struct Arena<'a> {
    /// All objects.
    pub store: &'a mut ObjectStore,
    /// The world's random stream (`RngSet::world`, PL-013): the only RNG of the pipeline.
    pub rng: &'a mut dyn GameRng,
    /// The game clock (`GetTickCount()` of the original).
    pub now: GameMs,
    /// The tick being processed.
    pub tick: Tick,
    /// The first frame the effects of this call take on the movers they touch (`Damage.server_frame`): the frame
    /// after the barrier that raised the hit (`F + 1`), `4n` in a handler, `4n + 4` after the frames. `None`
    /// reads each mover's own effect cursor ([`crate::movement::effect_frame`]).
    pub frame: Option<SimFrame>,
}

/// One message to send.
#[derive(Clone, Debug, PartialEq)]
pub struct Emit {
    /// The mover the message is about (`ServerMsg.obj_id` of the near audiences, the audience centre).
    pub subject: ObjId,
    /// Who receives it.
    pub aud: Aud,
    /// The message.
    pub body: pb::server_msg::Msg,
}

/// A call into the shared sim on one mover that needs the sim environment of the caller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimFx {
    /// `SendActMsg(OBJMSG_STAND)`.
    Stand,
    /// `ResetState(OBJSTA_MOVE_ALL)` (a sitting victim stands up).
    ResetMove,
    /// `SetDamagedMotion(attacker, atk_flags)`: `attacker` is the attacker's id and yaw.
    DamagedMotion {
        /// The `AF_*` of the hit.
        atk_flags: u32,
        /// The attacker and its angle in degrees (the knock-back direction).
        attacker: Option<(ObjId, f32)>,
    },
    /// `SendActMsg(OBJMSG_STOP)`.
    Stop,
    /// `SendActMsg(OBJMSG_DIE, cause, attacker)`: the death state, motion and corpse counter.
    Die {
        /// `OBJMSG_DAMAGE` or `OBJMSG_DAMAGE_FORCE` (the fly-back death).
        cause: u32,
        /// The killer and its yaw.
        attacker: Option<(ObjId, f32)>,
    },
    /// `m_pActMover->ClearState()` (a revival).
    ClearState,
    /// `ResetState(OBJSTA_ATK_ALL)` (the end of the yo-yo counter buff aborts the attack motion).
    ResetAttack,
    /// `SendActMsg(OBJMSG_STUN)` (`SetStun`: the groggy motion, no action).
    Stun,
    /// `SendActMsg(OBJMSG_RESURRECTION)` (the Resurrection skill was accepted).
    Resurrection,
    /// `CMover::StartCollecting()` without the mode bit: `ClearDest`, `OBJMSG_STOP`, `OBJMSG_COLLECT` (Plan/19 §14).
    StartCollecting,
    /// `CMover::StopCollecting()` without the mode bit: the no-action flags off, the action group reset, the
    /// move state stand, the `ETC` state flag off, the stand motion (Plan/19 §14).
    StopCollecting,
}

impl SimFx {
    /// Runs the call on `sim` with the sim context `cx` (environment of the frame the effect takes).
    pub fn apply(self, sim: &mut SimMover, cx: &mut Cx<'_>) {
        match self {
            SimFx::Stand => {
                let _ = flyff_sim::act::send_act_msg_cx(sim, cx, ActMsg::new(OBJMSG_STAND));
            }
            SimFx::ResetMove => sim.reset_state(cx, OBJSTA_MOVE_ALL),
            SimFx::DamagedMotion {
                atk_flags,
                attacker,
            } => {
                flyff_sim::act::damage::set_damaged_motion(sim, cx, atk_flags, attacker);
            }
            SimFx::Stop => {
                let _ = flyff_sim::act::send_act_msg_cx(sim, cx, ActMsg::new(OBJMSG_STOP));
            }
            SimFx::Die { cause, attacker } => {
                flyff_sim::act::damage::die(sim, cx, cause, attacker);
            }
            SimFx::ClearState => sim.clear_state(cx),
            SimFx::ResetAttack => sim.reset_state(cx, OBJSTA_ATK_ALL),
            SimFx::Stun => {
                let _ = flyff_sim::act::send_act_msg_cx(sim, cx, ActMsg::new(OBJMSG_STUN));
            }
            SimFx::Resurrection => {
                let _ = flyff_sim::act::send_act_msg_cx(
                    sim,
                    cx,
                    ActMsg::new(flyff_sim::types::OBJMSG_RESURRECTION),
                );
            }
            SimFx::StartCollecting => {
                sim.clear_dest();
                let _ = flyff_sim::act::send_act_msg_cx(sim, cx, ActMsg::new(OBJMSG_STOP));
                let _ = flyff_sim::act::send_act_msg_cx(
                    sim,
                    cx,
                    ActMsg::new(flyff_sim::types::OBJMSG_COLLECT),
                );
            }
            SimFx::StopCollecting => {
                use flyff_sim::types::{
                    ANILOOP_LOOP, MTI_STAND, MVRF_NOACTION, OBJSTA_ACTION_ALL, OBJSTA_STAND,
                    OBJSTAF_ETC,
                };
                sim.mvrf.remove(MVRF_NOACTION);
                sim.reset_state(cx, OBJSTA_ACTION_ALL);
                sim.set_move_state(cx, OBJSTA_STAND);
                sim.act.flags.remove(OBJSTAF_ETC);
                flyff_sim::anim::select::set_motion(sim, cx, MTI_STAND, ANILOOP_LOOP, 0);
            }
        }
    }
}

/// What a damage call wants done outside the movers (see the module documentation).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    /// Messages, in the order of the legacy sends.
    pub emits: Vec<Emit>,
    /// AI messages `(receiver, message)`, delivered through the movement mailbox.
    pub mail: Vec<(ObjId, AiMsg)>,
    /// Sim calls `(mover, call)`.
    pub sim: Vec<(ObjId, SimFx)>,
    /// Movers that reached 0 HP, in the order they died (`OnDied` of the arbiter, Plan/17 §9.1).
    pub deaths: Vec<KillCtx>,
    /// Monsters to create (a transformation, `CMonsterSkill::MonsterTransform`).
    pub spawns: Vec<crate::ai::MonsterSpawn>,
    /// Objects to delete (`CObj::Delete`: the source of a transformation).
    pub deletes: Vec<ObjId>,
    /// Skill work that needs the services (procs, the counter-attack skill, monster skill casts, buff removals):
    /// delivered into [`CombatQueues::skill_jobs`](super::world::CombatQueues) and run by
    /// [`crate::skill::cast::drain_jobs`] (Plan/17 §5.15, §6.11, §6.15; WP-SKILL-08, -13).
    pub skill_jobs: Vec<crate::skill::cast::SkillJob>,
}

impl Effects {
    /// A message for `aud` about `subject`.
    pub fn push(&mut self, subject: ObjId, aud: Aud, body: impl Into<pb::server_msg::Msg>) {
        self.emits.push(Emit {
            subject,
            aud,
            body: body.into(),
        });
    }

    /// A `NearInclSelf` message about `subject`.
    pub fn near(&mut self, subject: ObjId, body: impl Into<pb::server_msg::Msg>) {
        self.push(subject, Aud::NearInclSelf, body);
    }

    /// A message to `subject`'s own session only, wire `obj_id` 0 (`SetTargetNtf`, `DefinedText`, ...).
    pub fn to_self(&mut self, subject: ObjId, body: impl Into<pb::server_msg::Msg>) {
        self.push(subject, Aud::SelfZero, body);
    }

    /// Appends the messages of a stat-function [`Out`] about `subject`.
    pub fn extend_out(&mut self, subject: ObjId, mut out: Out) {
        for o in out.take() {
            self.emits.push(Emit {
                subject,
                aud: o.aud,
                body: o.body,
            });
        }
    }

    /// `true` when nothing was collected.
    pub fn is_empty(&self) -> bool {
        self.emits.is_empty()
            && self.mail.is_empty()
            && self.sim.is_empty()
            && self.deaths.is_empty()
            && self.spawns.is_empty()
            && self.deletes.is_empty()
            && self.skill_jobs.is_empty()
    }

    /// Appends everything of `other` after this value's entries.
    pub fn append(&mut self, other: Effects) {
        self.emits.extend(other.emits);
        self.mail.extend(other.mail);
        self.sim.extend(other.sim);
        self.deaths.extend(other.deaths);
        self.spawns.extend(other.spawns);
        self.deletes.extend(other.deletes);
        self.skill_jobs.extend(other.skill_jobs);
    }

    /// Delivers the effects in a handler context at the current frame (`4n`): see [`Effects::deliver_game_at`].
    pub fn deliver_game(self, ctx: &mut GameCtx<'_>) {
        let frame = ctx.clock.frame0;
        self.deliver_game_at(ctx, frame);
    }

    /// Delivers the effects in a handler context: the messages through the outbox, in order; the AI mail into the
    /// movement mailbox; the sim calls with an environment of `frame` built like the movement handlers build
    /// theirs (a mover outside a loaded world is skipped); the deaths into the queue of the death pipeline
    /// ([`super::death::process_pending`] runs them).
    pub fn deliver_game_at(self, ctx: &mut GameCtx<'_>, frame: SimFrame) {
        for e in self.emits {
            let mut o = Out::new();
            o.push(e.aud, e.body);
            o.flush(ctx, e.subject);
        }
        for (to, msg) in self.mail {
            crate::ai::mail(&mut ctx.worlds.movement, to, msg);
        }
        for (id, fx) in self.sim {
            apply_sim_fx_game_at(ctx, id, fx, frame);
        }
        ctx.worlds.movement.combat.queues.deaths.extend(self.deaths);
        ctx.worlds
            .movement
            .combat
            .queues
            .skill_jobs
            .extend(self.skill_jobs);
        // the monsters a transformation creates (linked by the next `_add`) and the objects it deletes
        for s in &self.spawns {
            let data = std::sync::Arc::clone(ctx.data);
            crate::ai::spawn::commit(ctx.worlds, &data, ctx.rng.world(s.world), s);
        }
        for id in self.deletes {
            ctx.worlds.delete(id);
        }
    }

    /// Delivers the effects in the world phase: the messages through the layer's sink (the viewer set of the
    /// subject, encoded once), the AI mail into the mailbox, the sim calls with the sim context `sc`, the deaths
    /// into `deaths`.
    pub fn deliver_phase(self, p: PhaseDelivery<'_, '_>) {
        let PhaseDelivery {
            store,
            rng,
            sc,
            sink,
            mail,
            deaths,
            spawns,
            deletes,
            skill_jobs,
        } = p;
        for e in self.emits {
            let Some(m) = store.mover(e.subject) else {
                continue;
            };
            let mut o = Out::new();
            o.push(e.aud, e.body);
            o.flush_sink(&mut *sink, m);
        }
        mail.extend(self.mail);
        for (id, fx) in self.sim {
            apply_sim_fx_phase(store, &mut *rng, sc, id, fx);
        }
        deaths.extend(self.deaths);
        spawns.extend(self.spawns);
        deletes.extend(self.deletes);
        skill_jobs.extend(self.skill_jobs);
    }
}

/// What [`Effects::deliver_phase`] writes to: the pieces of a [`crate::world::phases::FrameCtx`] (and the
/// combat world) it needs, borrowed separately.
pub struct PhaseDelivery<'a, 'b> {
    /// All objects.
    pub store: &'a mut ObjectStore,
    /// The world's random stream (the cosmetic draws of the sim calls).
    pub rng: &'a mut dyn GameRng,
    /// The sim context (the frame the effects start in).
    pub sc: &'a SimCtx<'b>,
    /// The layer's message sink.
    pub sink: &'a mut dyn SpawnSink,
    /// The AI mailbox.
    pub mail: &'a mut Vec<(ObjId, AiMsg)>,
    /// The death queue.
    pub deaths: &'a mut Vec<KillCtx>,
    /// The monsters to create once the hook returns ([`crate::world::phases::FrameCtx::spawn`]).
    pub spawns: &'a mut Vec<crate::ai::MonsterSpawn>,
    /// The objects to delete once the hook returns ([`crate::world::phases::FrameCtx::delete`]).
    pub deletes: &'a mut Vec<ObjId>,
    /// The skill jobs waiting for the service context ([`CombatQueues::skill_jobs`](super::world::CombatQueues)).
    pub skill_jobs: &'a mut Vec<crate::skill::cast::SkillJob>,
}

/// What a sim call needs from the layer besides the movers: the frame the effect starts in, the shared tables
/// (motion table, sim switches, dynamic colliders) and the collision world of the mover's world.
#[derive(Clone, Copy)]
pub struct SimCtx<'a> {
    /// The frame the call takes effect in (`F + 1` at the barrier of frame `F`).
    pub frame: SimFrame,
    /// The movement pipeline's shared data.
    pub shared: &'a MoveShared,
    /// The collision world of the layer's world.
    pub collision: &'a CollisionWorld,
    /// The dynamic ctrl colliders of the layer.
    pub dynamic: &'a DynamicColliders,
}

impl SimCtx<'_> {
    /// The sim environment of this context over `lookup`.
    pub fn env<'e>(&'e self, lookup: &'e dyn flyff_sim::step::MoverLookup) -> SimEnv<'e> {
        SimEnv {
            frame: self.frame,
            role: SimRole::Server,
            flags: self.shared.flags,
            world: self.collision,
            dynamic: self.dynamic,
            motions: &self.shared.motions,
            lookup,
            mode: EventMode::Normal,
        }
    }
}

/// Applies one [`SimFx`] to mover `id` in the world phase (the arena-based twin of [`apply_sim_fx_game_at`]).
pub fn apply_sim_fx_phase(
    store: &mut ObjectStore,
    rng: &mut dyn GameRng,
    sc: &SimCtx<'_>,
    id: ObjId,
    fx: SimFx,
) {
    let lookup = FrameLookup::for_objects(store, &[id], &sc.shared.flags);
    let Some(m) = store.mover_mut(id) else {
        return;
    };
    let env = sc.env(&lookup);
    let mut rules = NoRules;
    let Mover { sim, sync, .. } = m;
    let mut cx = Cx::new(&env, &mut rules, rng, &mut sync.events);
    fx.apply(sim, &mut cx);
}

/// Applies one [`SimFx`] to mover `id` in a handler context at the current frame (`4n`).
pub fn apply_sim_fx_game(ctx: &mut GameCtx<'_>, id: ObjId, fx: SimFx) {
    let frame = ctx.clock.frame0;
    apply_sim_fx_game_at(ctx, id, fx, frame);
}

/// Applies one [`SimFx`] to mover `id` in a handler context: the environment of `frame`, the world's collision
/// data and the mover's own event buffer.
pub fn apply_sim_fx_game_at(ctx: &mut GameCtx<'_>, id: ObjId, fx: SimFx, frame: SimFrame) {
    let shared = ctx.worlds.movement.shared.clone();
    let Some(statics) = ctx.worlds.statics_of(id).cloned() else {
        return;
    };
    let Some(world) = ctx.worlds.core(id).map(|c| c.world) else {
        return;
    };
    let layer = ctx.worlds.core(id).map(|c| c.layer).unwrap_or_default();
    let dynamic = ctx.worlds.dynamic_of(world, layer);
    let rng = ctx.rng.world(world);
    let sc = SimCtx {
        frame,
        shared: &shared,
        collision: &statics.collision,
        dynamic: &dynamic,
    };
    apply_sim_fx_phase(ctx.worlds.store_mut(), rng, &sc, id, fx);
}
