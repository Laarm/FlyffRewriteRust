//! Destination steering, arrival tests, destination angle and scripted patterns
//! (Plan/31 §11, §12.7; WP-SIM-06/10).
//!
//! Arrival tests and collision run on every frame for every mover; the steering messages
//! (`TURNMOVE`/`TURNMOVE2`, `approach_dest_angle`) run on every frame for players and only on frames
//! with `F % 4 == 0` for NPCs (Plan/31 §3.2 "steering cadence", PL-002/PL-004).

use crate::act::{ActMsg, send_act_msg_cx};
use crate::bounds::{is_range_obj_snap, radius};
use crate::mover::SimMover;
use crate::num::{
    angle_to_vector_xz, dot, ftoi_x86, get_degree, get_degree_x, normalize_or_zero,
    set_angle_quantized,
};
use crate::step::{Cx, SimEnv, SimEvent};
use crate::types::*;
use flyff_common::ObjId;

/// The steering cadence of Plan/31 §3.2 (S4, PL-002, PL-004): destination steering (`approach_dest_angle`
/// and the `TURNMOVE`/`TURNMOVE2` messages) runs on every frame for a player and only on the frames with
/// `F % 4 == 0` (the first sub-frame of a server tick) for every other mover; `F` is the server frame
/// number. Arrival tests and collision run on every frame for every mover.
#[inline]
pub fn steering_frame(m: &SimMover, frame: u64) -> bool {
    m.is_player() || frame.is_multiple_of(flyff_common::time::FRAMES_PER_TICK)
}

/// `ProcessAniSpeed` (Plan/31 §12.7): players animate attacks at their attack speed and casts at the
/// casting animation speed; everything else at 1.0.
pub fn ani_speed(m: &mut SimMover, env: &SimEnv<'_>) {
    m.ani_speed = if m.is_player() {
        match m.act.state.atk() {
            OBJSTA_ATK1 | OBJSTA_ATK2 | OBJSTA_ATK3 | OBJSTA_ATK4 | OBJSTA_ATK_MAGIC1
            | OBJSTA_ATK_RANGE1 => m.params.attack_speed,
            OBJSTA_ATK_CASTING1 | OBJSTA_ATK_CASTING2 | OBJSTA_ATK_MAGICSKILL => {
                m.params.casting_ani_speed
            }
            OBJSTA_ATK_MELEESKILL | OBJSTA_ATK_RANGESKILL if env.flags.speed_cast => {
                m.params.casting_ani_speed
            }
            _ => 1.0,
        }
    } else {
        1.0
    };
}

/// `CMover::ApproachGhostAngle` without the correction branch (Plan/31 §11.4): while a destination angle
/// is set, snap to it within a tolerance (`4.001`, flying `0.601`) and stop turning, otherwise keep
/// turning toward it (KEEP-QUIRK: no wrap-around).
pub fn approach_dest_angle(m: &mut SimMover, cx: &mut Cx<'_>) {
    if m.dest_angle > -0.001 {
        let fly = m.act.flags.fly();
        let tol = if fly { 0.601 } else { 4.001 };
        if (m.angle - m.dest_angle).abs() < tol {
            m.angle = set_angle_quantized(m.dest_angle);
            m.clear_dest_angle();
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP_TURN));
        } else {
            let p1 = if fly { 60 } else { 0 };
            let msg = if m.dest_left {
                OBJMSG_LTURN
            } else {
                OBJMSG_RTURN
            };
            send_act_msg_cx(m, cx, ActMsg::with(msg, [p1, 0, 0, 0, 0]));
        }
    }
}

/// `CMover::OnArriveAtPos` (without the ghost-correction branch): sends the arrival motion message,
/// reports the arrival and clears the destination position.
pub fn on_arrive_at_pos(m: &mut SimMover, cx: &mut Cx<'_>) {
    let msg = m.motion_arrive;
    send_act_msg_cx(m, cx, ActMsg::new(msg));
    cx.emit(SimEvent::ArrivedAtPos);
    m.clear_dest_pos();
}

/// `CMover::ProcessMoveArrival` (Plan/31 §11.2, server and non-active client branches): the destination
/// object was reached.
pub fn process_move_arrival(m: &mut SimMover, cx: &mut Cx<'_>, target: ObjId) {
    let cmd = m.cmd.act;
    if cx.server() {
        if cmd == OBJACT_USESKILL {
            // doc 17 performs DoUseSkill at the barrier and then clears the destination and stops
            cx.emit(SimEvent::ArrivedAtObj { target, cmd });
            return;
        }
        send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
        m.clear_dest_obj();
        cx.emit(SimEvent::ArrivedAtObj {
            target,
            cmd: OBJACT_NONE,
        });
        m.cmd.act = OBJACT_NONE;
    } else if m.is_active {
        // the active mover's command helpers (Plan/31 §11.7, WP-SIM-23) are driven by the client host
        cx.emit(SimEvent::CmdArrived { cmd, target });
    } else {
        m.clear_dest_obj();
        send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STOP));
    }
}

/// `CMover::ProcessMove` (Plan/31 §11.1; WP-SIM-06/10).
pub fn process_move(m: &mut SimMover, cx: &mut Cx<'_>) {
    let steer = steering_frame(m, cx.frame());
    if steer {
        approach_dest_angle(m, cx);
    }
    if m.is_empty_dest() || m.act.state.is_act_attack() {
        return;
    }
    let fly = m.act.flags.fly();
    let target;
    if !m.is_empty_dest_pos() {
        let px = (m.pos.x - m.dest.pos.x) > 0.0;
        let pz = (m.pos.z - m.dest.pos.z) > 0.0;
        if px != m.dest.positive_x || pz != m.dest.positive_z {
            if m.is_player() {
                // PL-004 (`__BS_FIX_ARRIVEPOS_ALGO` for player movers on both sides): arrive only when the
                // mover faces away from the destination after the sign flip
                let k = normalize_or_zero(m.pos - m.dest.pos);
                let f = normalize_or_zero(angle_to_vector_xz(m.angle, 1.0));
                if dot(k, f) > 0.0 {
                    on_arrive_at_pos(m, cx);
                    return;
                }
            } else {
                on_arrive_at_pos(m, cx);
                return;
            }
        }
        target = m.dest.pos;
    } else {
        let Some(t) = cx.env.lookup.snapshot(m.dest.obj) else {
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
            cx.emit(SimEvent::DestObjLost);
            return;
        };
        target = t.pos;
        let my_radius = radius(&m.params.bounds, m.params.scale);
        let in_range = is_range_obj_snap(&t, m.pos, my_radius, m.dest.arrival_range);
        if fly {
            if in_range {
                let id = m.dest.obj;
                let cmd = m.cmd.act;
                m.clear_dest_obj();
                cx.emit(SimEvent::ArrivedAtObj { target: id, cmd });
            }
        } else if in_range {
            let id = m.dest.obj;
            process_move_arrival(m, cx, id);
            return;
        }
    }
    if !fly && steer {
        if m.dest.forward {
            let a = ftoi_x86(get_degree(target, m.pos));
            let b = ftoi_x86(get_degree_x(target, m.pos));
            send_act_msg_cx(m, cx, ActMsg::with(OBJMSG_TURNMOVE, [a, b, 0, 0, 0]));
        } else {
            let a = ftoi_x86(get_degree(m.pos, target));
            let b = ftoi_x86(get_degree_x(m.pos, target));
            send_act_msg_cx(m, cx, ActMsg::with(OBJMSG_TURNMOVE2, [a, b, 0, 0, 0]));
        }
    }
}

/// `ProcessMovePattern` (Plan/31 §11.6): scripted move patterns of flying monsters (patterns 1 and 2).
pub fn process_move_pattern(m: &mut SimMover, cx: &mut Cx<'_>) {
    match m.pattern.pattern {
        1 => crate::act::pattern::process_pattern1(m, cx),
        2 => crate::act::pattern::process_pattern2(m, cx),
        _ => {}
    }
}
