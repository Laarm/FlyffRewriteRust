//! Per-frame kinematics (Plan/31 §8; WP-SIM-06): `CMover::GetSpeed` and `CActionMover::ProcessAction`.
//!
//! `process_action` runs the state-group processing, gravity and the external force and accumulates the
//! frame's movement into `delta_accu`; the position itself is changed by the collision pass
//! (`collision::ground::process`, step 7 of `step_frame`).

use crate::act::{ActMsg, send_act_msg_cx, state_fly, state_ground};
use crate::mover::SimMover;
use crate::num::{Vec3, length_sq};
use crate::step::{Cx, SimEnv, SimEvent};
use crate::types::*;

/// Downward acceleration per frame (`0.005` m/frame^2 = 18 m/s^2).
pub const GRAVITY: f32 = 0.005;
/// Terminal fall speed per frame (`0.5` m/frame = 30 m/s).
pub const TERMINAL_VELOCITY: f32 = 0.5;

/// `GetSpeed` without the pet rule: `base_speed * speed_factor`, rooted by a `DST_SPEED` change, plus the
/// `DST_SPEED` adjustment percent, clamped to be non-negative (`__JEFF_11`).
pub fn own_speed(m: &SimMover) -> f32 {
    let p = &m.params;
    let mut s = p.base_speed * p.speed_factor;
    if p.speed_chg_set {
        return 0.0;
    }
    if p.speed_adj_pct != 0 {
        s = s + s * p.speed_adj_pct as f32 / 100.0;
    }
    if s < 0.0 {
        s = 0.0;
    }
    s
}

/// `CMover::GetSpeed(m_fSpeed)` (`MoverParam.cpp:3303-3358`): with `improved_pets` a pet or egg copies the
/// speed of the mover it follows (`dest.obj`, start-of-frame snapshot); everything else is [`own_speed`].
pub fn get_speed(m: &SimMover, env: &SimEnv<'_>) -> f32 {
    // the client branch of `GetSpeed` for the local system pet (`CAIEgg`, `MoverParam.cpp:3306-3314`, WP-CLIENT-31): the speed of the owner
    // that the pet follows, whatever `features.improved_pets` says (the plan text ties it to the flag, the source does not)
    if m.params.ai == defs::AII_EGG
        && m.params.pet_owner != crate::mover::NO_OBJ
        && let Some(o) = env.lookup.snapshot(m.params.pet_owner)
    {
        return o.speed;
    }
    if env.flags.improved_pets
        && matches!(m.params.ai, defs::AII_PET | defs::AII_EGG)
        && let Some(o) = env.lookup.snapshot(m.dest.obj)
    {
        return o.speed;
    }
    own_speed(m)
}

/// `CActionMover::ProcessAction` (Plan/31 §8.1): state processing, gravity, external force; accumulates
/// `delta_accu`.
pub fn process_action(m: &mut SimMover, cx: &mut Cx<'_>) {
    let fly = m.act.flags.fly();
    if !fly {
        m.act.delta.x = 0.0;
        m.act.delta.z = 0.0;
    }
    if m.act.state.bits() == 0 {
        send_act_msg_cx(m, cx, ActMsg::new(OBJMSG_STAND));
        if cx.client() && m.is_active && m.act.flags.fly() {
            cx.emit(SimEvent::FlyMoved); // `SendPlayerMoved2()`
        }
    } else {
        let speed = get_speed(m, cx.env);
        // the groups are re-read from the live state word between groups
        for mask in [
            OBJSTA_MOVE_ALL,
            OBJSTA_TURN_ALL,
            OBJSTA_LOOK_ALL,
            OBJSTA_JUMP_ALL,
            OBJSTA_ATK_ALL,
            OBJSTA_DMG_ALL,
            OBJSTA_ACTION_ALL,
        ] {
            let g = m.act.state.bits() & mask;
            if g != 0 {
                // `ProcessState`: the flying table iff the FLY flag is set right now
                if m.act.flags.fly() {
                    state_fly::process_state(m, cx, g, speed);
                } else {
                    state_ground::process_state(m, cx, g, speed);
                }
            }
        }
    }
    let a = &m.act;
    m.act.moving = !(a.delta.x == 0.0
        && a.delta.z == 0.0
        && a.delta_e.x == 0.0
        && a.delta_e.y == 0.0
        && a.delta_e.z == 0.0);
    if m.act.delta.y != 0.0 || m.act.moving || (m.pos.y - m.act.pos_last.y).abs() > 0.001 {
        m.act.ground = 0;
    }
    if m.act.flags.fly() {
        crate::fly::process_fly_move(m, cx);
    } else if !m.params.flying_npc && m.act.state.dmg() != OBJSTA_DISAPPEAR && m.act.ground != 1 {
        m.act.delta.y -= GRAVITY;
        if m.act.delta.y < -TERMINAL_VELOCITY {
            m.act.delta.y = -TERMINAL_VELOCITY;
        }
    }
    if m.act.state.is_die() {
        m.act.delta_e = Vec3::ZERO;
    }
    if cx.client()
        && (m.act.delta_e.x >= 0.025 || m.act.delta_e.z >= 0.025)
        && m.act.ground != 0
        && cx.frame() & 1 != 0
    {
        cx.emit(SimEvent::DustFx);
    }
    m.act.delta += m.act.delta_e;
    m.act.delta_accu += m.act.delta;
    if m.act.state.bits() & OBJSTA_DMG_FLY == 0 {
        if length_sq(m.act.delta_e) < 0.01 {
            m.act.delta_e = Vec3::ZERO;
        }
        m.act.delta_e = Vec3::new(
            m.act.delta_e.x * 0.9,
            m.act.delta_e.y * 0.9,
            m.act.delta_e.z * 0.9,
        );
    }
    m.act.count += 1;
}
