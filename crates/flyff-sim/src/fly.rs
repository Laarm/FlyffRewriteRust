//! The player flying model: `ProcessFlyMove` (thrust, turbo fuel, drift brake, speed cap, air friction,
//! stop) and `ProcessFlyTracking` (Plan/31 §9; ref/12 §5.8; WP-SIM-25).
//!
//! Flying players ride a board or broom (`params.ride`). Per frame the state processing of
//! [`crate::act::state_fly`] sets `acc_power` (`flight_speed * 0.75` while moving forward, `0` standing) and
//! the pitch/yaw, then [`process_fly_move`] turns the thrust into a velocity change; the fly collision pass
//! (`collision::ground`, Plan/31 §10.9) applies the accumulated movement. Everything here is `f32` in the
//! operation order of `src:_AIInterface/ActionMoverState2.cpp:376-478`, so the result is identical on every
//! target.

use crate::act::{ActMsg, send_act_msg_cx};
use crate::mover::SimMover;
use crate::num::{
    Vec3, cosf, dot, ftoi_x86, length_sq, normalize_or_zero, set_angle_quantized, sinf, to_radian,
    x_get_degree,
};
use crate::step::{Cx, SimEvent};
use crate::types::*;

/// `FRIC_AIR`: the fraction of the velocity lost every frame.
pub const FRIC_AIR: f32 = 0.011;
/// `ride.flight_speed * ACC_POWER_FACTOR` is the thrust of `FMOVE` (`ProcessState2`).
pub const ACC_POWER_FACTOR: f32 = 0.75;
/// The thrust factor of turbo (`fAccPwr *= 1.2f`; the client's old prediction used 1.5, the server 1.2).
pub const TURBO_ACC_FACTOR: f32 = 1.2;
/// The speed above which thrust is not added: `vmax = 0.3` per frame (18 m/s).
pub const MAX_SPEED: f32 = 0.3;
/// Turbo raises the speed cap by this factor.
pub const TURBO_MAX_FACTOR: f32 = 1.1;
/// The drift test: `dot(normalize(v), normalize(a)) < 0.633319` ("`cosf(70.0f)`", about 50.7 degrees).
pub const DRIFT_COS: f32 = 0.633_319;
/// Turbo fuel per frame in milliseconds: `1000.0f / FRAME_PER_SEC`.
pub const TURBO_FUEL_MS_PER_FRAME: f32 = 1000.0_f32 / 60.0_f32;
/// The squared speed below which a coasting flyer stops: `0.0002f * 0.0002f`.
pub const STOP_SPEED_SQ: f32 = 0.0002_f32 * 0.0002_f32;
/// The pitch limit of the look states and of mouse steering, degrees.
pub const MAX_PITCH_DEG: f32 = 45.0;

/// `CActionMover::ProcessFlyMove` (Plan/31 §9.2), called from `process_action` for every mover with the FLY
/// flag. Changes `delta` (the per-frame velocity), the turbo fuel and the ACC/TURBO flags; the position is
/// changed by the fly collision pass.
///
/// * nothing happens while the velocity is zero and `ACC` is off;
/// * the active client mover first runs the auto-tracking steering (`MVRF_TRACKING`);
/// * with `TURBO` and `ACC`, `tm_acc_fuel` drops by `ftoi(1000/60)` ms per frame (**FIX**: the original
///   decremented it on the server only, the client waited for the broadcast); at `<= 0` it is clamped to 0
///   and `MODE_TURBO_OFF` is sent (event [`SimEvent::TurboOff`], the server broadcasts it), otherwise the
///   thrust is multiplied by `1.2`;
/// * thrust `a = (sin(yaw) cos(pitch), -sin(pitch), -cos(yaw) cos(pitch)) * acc`; when the speed exceeds
///   `sqrt(0.01)` and the angle between velocity and thrust is above about 50.7 degrees the drift brake
///   doubles the thrust and multiplies the velocity by `0.985` (client event `DriftFx`); thrust is only
///   added below the speed cap `0.3` (`0.33` with turbo);
/// * the velocity always loses `FRIC_AIR`; a flyer whose `acc_power` is 0 and whose speed fell below
///   `0.0002` stops dead and leaves the `ACC` state.
pub fn process_fly_move(m: &mut SimMover, cx: &mut Cx<'_>) {
    let len_sq = length_sq(m.act.delta);
    if len_sq == 0.0 && !m.act.flags.has(OBJSTAF_ACC) {
        return;
    }
    if cx.client() && m.is_active && m.mvrf.has(MVRF_TRACKING) {
        fly_tracking(m, cx);
    }
    let mut acc = m.act.acc_power;
    // turbo
    if m.act.flags.has(OBJSTAF_TURBO) && m.act.flags.has(OBJSTAF_ACC) {
        m.tm_acc_fuel = ftoi_x86(m.tm_acc_fuel as f32 - TURBO_FUEL_MS_PER_FRAME);
        if m.tm_acc_fuel <= 0 {
            m.tm_acc_fuel = 0;
            send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_MODE_TURBO_OFF));
            cx.emit(SimEvent::TurboOff);
        } else {
            acc *= TURBO_ACC_FACTOR;
        }
    }
    // thrust
    if acc > 0.0 {
        let ax = to_radian(m.angle_x);
        let ay = to_radian(m.angle);
        let d = cosf(ax) * acc;
        let mut a = Vec3::new(sinf(ay) * d, -sinf(ax) * acc, -cosf(ay) * d);
        if len_sq > 0.01 {
            let vn = normalize_or_zero(m.act.delta);
            let an = normalize_or_zero(a);
            if dot(vn, an) < DRIFT_COS {
                a = Vec3::new(a.x * 2.0, a.y * 2.0, a.z * 2.0);
                m.act.delta = Vec3::new(
                    m.act.delta.x * 0.985,
                    m.act.delta.y * 0.985,
                    m.act.delta.z * 0.985,
                );
                if cx.client() {
                    cx.emit(SimEvent::DriftFx);
                }
            }
        }
        let l2 = length_sq(m.act.delta);
        let mut vmax = MAX_SPEED;
        if m.act.flags.has(OBJSTAF_TURBO) {
            vmax *= TURBO_MAX_FACTOR;
        }
        if l2 < vmax * vmax {
            m.act.delta += a;
        }
    }
    // air friction
    let keep = 1.0_f32 - FRIC_AIR;
    m.act.delta = Vec3::new(
        m.act.delta.x * keep,
        m.act.delta.y * keep,
        m.act.delta.z * keep,
    );
    // a coasting flyer that has (nearly) stopped stands still and leaves the ACC state
    if m.act.acc_power == 0.0 && length_sq(m.act.delta) < STOP_SPEED_SQ {
        m.act.delta = Vec3::ZERO;
        m.act.flags.remove(OBJSTAF_ACC);
    }
}

/// `wrap180` of `ProcessFlyTracking`: a single step, `> 180 -> -360`, `< -180 -> +360`.
fn wrap180(x: f32) -> f32 {
    if x > 180.0 {
        x - 360.0
    } else if x < -180.0 {
        x + 360.0
    } else {
        x
    }
}

/// `CActionMover::ProcessFlyTracking` (Plan/31 §9.3; client, active mover with `MVRF_TRACKING`): steers the
/// yaw by `+-2.5` degrees per frame and the pitch by `+-1.5` towards the tracked target (dead zone 3
/// degrees); a change raises [`SimEvent::FlyAngle`] (`PostPlayerAngle(TRUE)`). Returns whether it steered.
pub fn fly_tracking(m: &mut SimMover, cx: &mut Cx<'_>) -> bool {
    let Some(t) = cx.env.lookup.snapshot(m.tracking) else {
        return false;
    };
    let dist = t.pos - m.pos;
    let (ang_xz, ang_h) = x_get_degree(dist);
    let mut mover_ang = m.angle;
    if mover_ang > 180.0 {
        mover_ang -= 360.0;
    }
    let sub = wrap180(ang_xz - mover_ang);
    let turn = if sub < -3.0 {
        -2.5_f32
    } else if sub > 3.0 {
        2.5_f32
    } else {
        0.0_f32
    };
    let sub_h = wrap180(ang_h - m.angle_x);
    let turn_h = if sub_h < -3.0 {
        -1.5_f32
    } else if sub_h > 3.0 {
        1.5_f32
    } else {
        0.0_f32
    };
    m.angle = set_angle_quantized(m.angle + turn);
    m.angle_x += turn_h;
    let steered = turn != 0.0 || turn_h != 0.0;
    if steered {
        cx.emit(SimEvent::FlyAngle);
    }
    steered
}
