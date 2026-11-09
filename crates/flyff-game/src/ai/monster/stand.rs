//! `StateStand` and `MoveProcessStand` (Plan/18 §9.5, ref/12 §9.3; `AIMonster.cpp:579-698`, `779-914`;
//! WP-AI-08): IDLE without the wandering, with a 0.1 m return radius, a 10 s warp home and the turn to the
//! spawn angle.

use super::helpers::{is_in_range, mover_is_die};
use super::idle::{arrival, reset_pattern, scan_and_engage};
use super::{
    CHASE_TIMER_MS, MonsterAi, MonsterState, RANGE_MOVE, RETURN_DONE_RANGE_STAND,
    RETURN_WARP_AFTER_STAND_MS,
};
use crate::ai::ctx::AiCx;
use crate::ai::msg::AiMsg;
use crate::movement::{IntentAudience, PendingIntent, intents};
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_sim::num::set_angle_quantized;
use flyff_sim::types::OBJSTA_DMG_FLY_ALL;

/// The messages of `STATE_STAND`.
pub(super) fn handle(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
    match msg {
        AiMsg::Init { .. } => {
            let mut range = RANGE_MOVE;
            if m.sim.params.flying_npc {
                range *= 3.0;
            }
            if !is_in_range(m.sim.pos - ai.spawn_pos, range) {
                ai.do_return_to_begin(m, cx, true);
            }
            ai.set_stop(m, cx, 0);
        }
        AiMsg::InitTargetClear => ai.set_target(m, None, 0),
        AiMsg::Process => move_process_stand(ai, m, cx),
        AiMsg::Damage { attacker, .. } => {
            if !ai.returning {
                ai.move_to_obj(m, cx, attacker);
                ai.send(
                    m,
                    cx,
                    AiMsg::SetState {
                        state: MonsterState::Rage as u32,
                        param: Some(attacker),
                    },
                );
                ai.tm_attack = GameMs(cx.now.0 + CHASE_TIMER_MS);
                ai.pos_damage = Some(m.sim.pos);
            }
        }
        AiMsg::Die => ai.send(m, cx, AiMsg::Exit),
        // `AIMSG_COLLISION`: the IDLE side step is commented out in the original
        AiMsg::Collision => {}
        AiMsg::Arrival { pos_arrival, .. } => arrival(ai, m, cx, pos_arrival),
        _ => {}
    }
}

/// `MoveProcessStand` (18 §9.5).
fn move_process_stand(ai: &mut MonsterAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    if mover_is_die(m) || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
        return;
    }
    if ai.returning {
        if is_in_range(ai.spawn_pos - m.sim.pos, RETURN_DONE_RANGE_STAND) {
            ai.do_return_to_begin(m, cx, false);
            // the server turns at once; the clients turn gradually toward the announced angle
            m.sim.angle = set_angle_quantized(ai.spawn_angle);
            intents::enqueue(
                m,
                PendingIntent::DestAngle {
                    angle: ai.spawn_angle,
                    left: false,
                },
                IntentAudience::Viewers,
            );
            ai.send(
                m,
                cx,
                AiMsg::Arrival {
                    obj: None,
                    pos_arrival: false,
                },
            );
        }
        // the 0.1 m radius is rarely met: usually the monster is put home after 10 s (KQ-13 Q4)
        if cx.now.0.saturating_sub(ai.tm_return.0) >= RETURN_WARP_AFTER_STAND_MS {
            ai.warp_home(m, cx);
        }
        return;
    }
    if m.sim.pattern.pattern != 0 && m.sim.pattern.event == 99 {
        reset_pattern(m);
    }
    scan_and_engage(ai, m, cx);
}
