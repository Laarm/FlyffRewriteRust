//! The sim-event dispatch table of the barrier (Plan/18 §3.4, §4.5; WP-AI-01).
//!
//! After every frame `F` the pipeline drains the events of every awake mover in slot order and reacts; what a
//! reaction changes is effective from frame `F + 1` and every message it queues is stamped `F + 1`. The
//! movement code reacts to the events that concern movement (arrivals, collisions, drops, begin of a move, the
//! end of the appear scene and of a monster melee, the corpse that may go); everything else is only passed
//! on to [`MoveConsumers::sim_event`](super::MoveConsumers::sim_event).
//!
//! | event | non-player | player |
//! |---|---|---|
//! | `ArrivedAtPos` | AI `Arrival{None, pos}` | nothing |
//! | `ArrivedAtObj{target, cmd}` | AI `Arrival{Some(target)}` | `OBJACT_USESKILL`: 17's hook, then the action is cleared and the player stops; else 19's hook |
//! | `Collided` | NPC correction (§5.5) and AI `Collision` when armed | nothing (PL-027) |
//! | `AttrBlocked` | AI `Collision` | nothing |
//! | `Dropped` | NPC correction, no AI message | nothing |
//! | `BeganMove`, `AppearEnded`, `EndMeleeAttack` | the matching AI message | - |
//! | `TurboOff` | - | `SendActMsg{MODE_TURBO_OFF}` to the viewers (18 §6) |
//! | `EnteredNoFly` | - | the ride comes off after the world phase (18 §6, `flying::finish`) |
//! | `Disappeared` | the NPC is deleted (`CObj::Delete`) | - |
//!
//! At most one `AIMSG_COLLISION` is delivered per mover and frame (`Collided` and `AttrBlocked` coalesce).

use super::corrections;
use super::pipeline::MovementHooks;
use crate::ai::AiMsg;
use crate::world::object::MoverKey;
use crate::world::phases::FrameCtx;
use flyff_common::SimFrame;
use flyff_sim::ActMsg;
use flyff_sim::step::SimEvent;
use flyff_sim::types::{OBJACT_NONE, OBJACT_USESKILL, OBJMSG_STOP};
use smallvec::SmallVec;

/// At most one `AIMSG_COLLISION` per mover and frame: `Some` for the first wanted one.
fn coalesce(wanted: bool, sent: &mut bool) -> Option<AiMsg> {
    if wanted && !*sent {
        *sent = true;
        Some(AiMsg::Collision)
    } else {
        None
    }
}

impl MovementHooks {
    /// The barrier of frame `frame`: moves the effect cursor of every awake mover to `frame + 1`, drains the
    /// events in slot order and dispatches them.
    pub(super) fn barrier_events(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        frame: SimFrame,
        awake: &[MoverKey],
    ) {
        let next = SimFrame(frame.0 + 1);
        self.post = None;
        for &key in awake {
            let events: SmallVec<[SimEvent; 8]> = {
                let Some(m) = ctx.store.movers.get_mut(key) else {
                    continue;
                };
                m.sync.pre_tick = ctx.tick.0;
                m.sync.next_pre_frame = next.0;
                if m.sync.events.is_empty() {
                    continue;
                }
                m.sync.events.drain().collect()
            };
            ctx.movement.stats.events += events.len() as u64;
            let mut collision_sent = false;
            for ev in &events {
                self.dispatch(ctx, awake, key, frame, ev, &mut collision_sent);
            }
        }
        // the consumers' end of the barrier (the projectile phase of doc 17), then what they mailed to the AIs
        // is heard now (effective from F + 1)
        self.consumers.frame_end(ctx, frame);
        self.flush_mail(ctx, next, Some(awake));
    }

    fn dispatch(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        awake: &[MoverKey],
        key: MoverKey,
        frame: SimFrame,
        ev: &SimEvent,
        collision_sent: &mut bool,
    ) {
        let next = SimFrame(frame.0 + 1);
        let (id, is_player) = match ctx.store.movers.get(key) {
            Some(m) => (m.core.id, m.is_player()),
            None => return,
        };
        let concerns_movement = matches!(
            ev,
            SimEvent::Collided
                | SimEvent::AttrBlocked
                | SimEvent::Dropped
                | SimEvent::ArrivedAtPos
                | SimEvent::ArrivedAtObj { .. }
                | SimEvent::BeganMove
                | SimEvent::AppearEnded
                | SimEvent::EndMeleeAttack
        );
        // an eat pet only queues its arrivals (18 §12): it is never corrected and has no collision reaction
        let is_pet = !is_player
            && ctx
                .store
                .movers
                .get_mut(key)
                .is_some_and(|m| crate::ai::pet::is_pet(m));
        if is_pet && let Some(m) = ctx.store.movers.get_mut(key) {
            crate::ai::pet::on_event(m, ev);
        }
        if concerns_movement && !is_player && !is_pet {
            // the after-step snapshot of the whole layer: what a reaction (a `STAND`, an attack message) reads
            if self.post.is_none() {
                self.post = Some(super::FrameLookup::capture(
                    ctx.store,
                    awake,
                    &ctx.movement.shared.flags,
                ));
            }
            self.with_ai_cx(ctx, next, key, |ai, m, cx| {
                let msg = match ev {
                    SimEvent::Collided => {
                        let armed = corrections::on_collided(m, cx);
                        coalesce(armed, collision_sent)
                    }
                    SimEvent::AttrBlocked => coalesce(true, collision_sent),
                    SimEvent::Dropped => {
                        corrections::on_dropped(m, cx);
                        None
                    }
                    SimEvent::ArrivedAtPos => Some(AiMsg::Arrival {
                        obj: None,
                        pos_arrival: true,
                    }),
                    SimEvent::ArrivedAtObj { target, .. } => Some(AiMsg::Arrival {
                        obj: Some(*target),
                        pos_arrival: false,
                    }),
                    SimEvent::BeganMove => Some(AiMsg::BeginMove),
                    SimEvent::AppearEnded => Some(AiMsg::EndAppear),
                    SimEvent::EndMeleeAttack => Some(AiMsg::EndMeleeAttack),
                    _ => None,
                };
                if let (Some(msg), Some(ai)) = (msg, ai) {
                    ai.message(m, msg, cx);
                }
            });
            self.drain_cmds(ctx, next);
        }
        super::flying::on_event(ctx, key, id, is_player, ev);
        match ev {
            SimEvent::ArrivedAtObj { target, cmd } if is_player => {
                if *cmd == OBJACT_USESKILL {
                    self.consumers.player_skill_arrival(ctx, id, *target);
                    // the approach is over: forget the action, drop the destination and stop (`m_oaCmd`)
                    if self.post.is_none() {
                        self.post = Some(super::FrameLookup::capture(
                            ctx.store,
                            awake,
                            &ctx.movement.shared.flags,
                        ));
                    }
                    self.with_ai_cx(ctx, next, key, |_, m, cx| {
                        m.sim.cmd.act = OBJACT_NONE;
                        m.sim.clear_dest_obj();
                        cx.send_act(m, ActMsg::new(OBJMSG_STOP));
                    });
                } else {
                    self.consumers.player_arrived_at_obj(ctx, id, *target);
                }
            }
            SimEvent::Disappeared if !is_player => ctx.delete(id),
            _ => {}
        }
        self.consumers.sim_event(ctx, id, frame, ev);
    }
}
