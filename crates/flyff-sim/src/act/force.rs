//! The forced-set family: `ActionForceSet`, `ActionForceSet2`, `HalfForceSet` (Plan/31 §6.5;
//! `src:_AIInterface/ActionMoverMsg.cpp:35-93`, `src:_Common/MoverParam.cpp:3369-3440`; WP-SIM-04).
//!
//! Used for `PlayerMoveState` (ground), `MoverStateNtf` and server-forced poses: position and angle are
//! always applied, the state word is replaced without running end handlers, and the motion is re-set.

use crate::anim::select::{set_blending, set_motion};
use crate::mover::SimMover;
use crate::num::set_angle_quantized;
use crate::step::Cx;
use crate::sync::StateUpdate;
use crate::types::*;

/// `CActionMover::ForcedSet`: replaces velocity, state word, flags and `motion_ex`; REFUSED (nothing
/// changes) when the FLY or the SIT flag would flip (KEEP-QUIRK: those transitions go through
/// `MODE_FLY`/`MODE_GROUND`/`SITDOWN`).
fn forced_set(m: &mut SimMover, s: &StateUpdate) {
    let cur = m.act.flags;
    let new = s.flags;
    if cur.fly() != new.fly() || cur.sit() != new.sit() {
        return;
    }
    m.act.delta = s.delta;
    m.act.state = s.state; // __ForceSetState: no end handlers
    m.act.flags = new;
    m.act.motion_ex = s.motion_ex;
}

/// `CMover::ActionForceSet` (Plan/31 §6.5): ground state sync. Clears the destination position (the
/// destination object survives, `__SYNC_1217`) and the destination angle, applies position and angle, then
/// the forced set (refused on a FLY/SIT flag mismatch), forces the motion (`motion.id = -1`), switches
/// blending off and phase-aligns the clock when `motion_frame` is given.
pub fn action_force_set(m: &mut SimMover, cx: &mut Cx<'_>, s: &StateUpdate) {
    m.clear_dest_pos();
    m.clear_dest_angle();
    m.pos = s.pos;
    m.angle = set_angle_quantized(s.angle);
    forced_set(m, s);
    m.motion.id = u32::MAX;
    set_motion(m, cx, s.motion, s.loop_, s.motion_option);
    set_blending(m, false);
    if let Some(f) = s.motion_frame {
        m.motion.clock.set_frame(f);
    }
}

/// `CMover::ActionForceSet2` (flying): clears both destinations, sets pitch, `acc_power` and `turn_angle`;
/// refuses only on a FLY mismatch; on the server a `MTI_FSTAND1_A` motion (31) without `MOP_FIXED` gets
/// `MOP_FIXED` (KEEP-QUIRK).
pub fn action_force_set2(m: &mut SimMover, cx: &mut Cx<'_>, s: &StateUpdate) {
    m.clear_dest();
    m.clear_dest_angle();
    m.pos = s.pos;
    m.angle = set_angle_quantized(s.angle);
    if let Some(f) = s.fly {
        m.angle_x = f.angle_x;
    }
    if m.act.flags.fly() == s.flags.fly() {
        m.act.delta = s.delta;
        if let Some(f) = s.fly {
            m.act.acc_power = f.acc_power;
            m.act.turn_angle = f.turn_angle;
        }
        m.act.state = s.state;
        m.act.flags = s.flags;
        m.act.motion_ex = s.motion_ex;
    }
    m.motion.id = u32::MAX;
    let mut opt = s.motion_option;
    if cx.server() && s.motion == MTI_FSTAND1_A && opt & MOP_FIXED == 0 {
        opt |= MOP_FIXED;
    }
    set_motion(m, cx, s.motion, s.loop_, opt);
    set_blending(m, false);
    if let Some(f) = s.motion_frame {
        m.motion.clock.set_frame(f);
    }
}

/// `CMover::HalfForceSet`: position, angles, velocity, `acc_power` and `turn_angle` only (no state, no
/// motion); clears both destinations and the destination angle.
pub fn half_force_set(m: &mut SimMover, s: &StateUpdate) {
    m.clear_dest();
    m.clear_dest_angle();
    m.pos = s.pos;
    m.angle = set_angle_quantized(s.angle);
    if let Some(f) = s.fly {
        m.angle_x = f.angle_x;
        m.act.acc_power = f.acc_power;
        m.act.turn_angle = f.turn_angle;
    }
    m.act.delta = s.delta;
}
