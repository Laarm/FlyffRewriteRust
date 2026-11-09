//! Per-frame state processing of a flyer (`ProcessState2`, `src:_AIInterface/ActionMoverState2.cpp:485-585`;
//! Plan/31 §7.3; ref/12 §4.9; WP-SIM-25).
//!
//! Called once per state group by `kin::process_action` for a mover with the `FLY` flag. The groups set
//! the thrust (`acc_power`), turn and pitch the mover and select the ride's fly motions; the velocity itself
//! is `fly::process_fly_move`. Without ride parameters nothing is processed (the original logged an error
//! and returned); `RideMissing` is raised once.

use super::msg_fly::{report_missing, ride_motion};
use crate::anim::attr::is_attr_hit;
use crate::anim::select::set_motion;
use crate::fly::ACC_POWER_FACTOR;
use crate::mover::SimMover;
use crate::num::add_angle;
use crate::step::{Cx, HitKind, SimEvent};
use crate::types::*;

/// `ProcessState2(dw, speed)`: the behaviour of one state-group value for one frame of a flyer.
pub fn process_state(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32, speed: f32) {
    let Some(ride) = m.params.ride else {
        report_missing(m, cx);
        return;
    };
    let pitch = m.angle_x; // `fAng`: read once, before the group runs
    match dw {
        OBJSTA_STAND => {
            m.act.acc_power = 0.0; // standing or hovering: no thrust any more
            let st = m.act.state;
            if st.is_act_attack() || st.is_act_turn() || st.is_act_damage() {
                return;
            }
            ride_motion(m, cx, &ride, MTA_FSTAND1, ANILOOP_LOOP);
        }
        OBJSTA_FMOVE => {
            m.act.acc_power = ride.flight_speed * ACC_POWER_FACTOR;
            let st = m.act.state;
            if st.is_act_attack() || st.is_act_turn() {
                return;
            }
            ride_motion(m, cx, &ride, MTA_FRUNNING1, ANILOOP_LOOP);
        }
        OBJSTA_LTURN | OBJSTA_RTURN => {
            let left = dw == OBJSTA_LTURN;
            let mut t = m.act.turn_angle;
            if !m.act.flags.has(OBJSTAF_ACC) {
                t *= 2.5; // coasting turns two and a half times as fast
            }
            m.angle = add_angle(m.angle, if left { t } else { -t });
            if m.act.state.is_act_attack() {
                return;
            }
            let mta = if left { MTA_FLTURN1 } else { MTA_FRTURN1 };
            set_motion(
                m,
                cx,
                ride.use_motion.wrapping_add(mta),
                ANILOOP_LOOP,
                MOP_FIXED,
            );
        }
        OBJSTA_LOOKUP => {
            if pitch > -45.0 {
                m.angle_x += -ride.tb_angle;
            }
        }
        OBJSTA_LOOKDOWN => {
            if pitch < 45.0 {
                m.angle_x += ride.tb_angle;
            }
        }
        OBJSTA_ATK1 => process_atk1(m, cx, dw),
        // ATK2..ATK4 have no case in `_ProcessStateAttack2`
        OBJSTA_ATK2 | OBJSTA_ATK3 | OBJSTA_ATK4 => {}
        // the wand swing is the ground one (`ProcessStateAttackMagic`)
        OBJSTA_ATK_MAGIC1 => super::state_ground::process_state(m, cx, dw, speed),
        OBJSTA_DMG => {
            if m.motion.clock.end_frame {
                m.reset_state(cx, OBJSTA_DMG_ALL);
            }
        }
        OBJSTA_DEAD if cx.server() && !m.is_player() => {
            m.act.dead_cnt -= 1;
            // KEEP-QUIRK: `< 0` here, the ground table uses `<= 0`
            if m.act.dead_cnt < 0 {
                m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DISAPPEAR);
                m.act.count = 0;
                cx.emit(SimEvent::DeadCountExpired);
            }
        }
        _ => {}
    }
}

/// `_ProcessStateAttack2(ATK1)`: the end of the swing resets the attack group; unless the clock is paused
/// (the client's hit-stop) a hit frame asks doc 17 to apply the damage (`AttrHit{FlyMelee}`, the original
/// `target->SendDamage(AF_GENERIC, me)`). The client pauses a swinging player 5 frames after a hit; the
/// server never does (D4, KQ-04: the original server paused here too).
fn process_atk1(m: &mut SimMover, cx: &mut Cx<'_>, dw: u32) {
    if m.motion.clock.end_frame {
        m.reset_state(cx, OBJSTA_ATK_ALL);
    }
    if m.motion.clock.pause > 0 {
        m.motion.clock.pause -= 1;
        return;
    }
    let clip = m.motion.clip.and_then(|c| cx.env.motions.clip(c));
    if !is_attr_hit(&m.motion.clock, clip, cx.env.flags.hit_attr_rule) {
        return;
    }
    let target = m.act.objid_hit;
    if !cx.env.lookup.snapshot(target).is_some_and(|t| t.alive) {
        return;
    }
    cx.emit(SimEvent::AttrHit {
        kind: HitKind::FlyMelee,
        state: dw,
        target,
        hit_index: 0,
        frame: cx.frame(),
    });
    if cx.client() && m.is_player() {
        m.motion.clock.pause = 5;
    }
}
