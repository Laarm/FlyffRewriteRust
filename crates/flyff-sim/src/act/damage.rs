//! Knock-back and damage motions (Plan/31 §8.6, §6.2 DIE; WP-SIM-04/08/20 parts).
//!
//! The damage *rules* (hit rolls, damage numbers, `PreProcessDamage`) belong to doc 17; the sim provides
//! the motion/state side: [`do_damage_fly`], [`set_damaged_motion`] and [`die`].

use super::ActMsg;
use crate::anim::select::set_motion;
use crate::mover::SimMover;
use crate::num::{DEG2RAD, cosf, sinf};
use crate::step::{Cx, SimEvent};
use crate::types::*;
use flyff_common::ObjId;
use flyff_common::rng::GameRng;

/// `CActionMover::DoDamageFly(ang_xz, ang_y, p)` (Plan/31 §8.6): adds the knock-back impulse
/// `delta.y += -cos(t2) * p`, `delta_e.xz += (sin t, -cos t) * (sin(t2) * p)` with `t = ang_xz`, `t2 = ang_y`
/// in degrees. With `ang_y = 145`: `delta.y += 0.819 p`, horizontal `0.574 p`.
pub fn do_damage_fly(m: &mut SimMover, ang_xz: f32, ang_y: f32, p: f32) {
    let t = ang_xz * DEG2RAD;
    let t2 = ang_y * DEG2RAD;
    m.act.delta.y += -cosf(t2) * p;
    let d = sinf(t2) * p;
    m.act.delta_e.x += sinf(t) * d;
    m.act.delta_e.z += -cosf(t) * d;
}

/// `CMover::SetDamagedMotion` (`MoverAttack.cpp:2204-2256`; Plan/31 §8.6): enters the damage state/motion
/// for a hit with attack flags `atk_flags`. `attacker` is the attacker's id and yaw (for the flying
/// knock-back). Does nothing while already in a knock-back state.
pub fn set_damaged_motion(
    m: &mut SimMover,
    cx: &mut Cx<'_>,
    atk_flags: u32,
    attacker: Option<(ObjId, f32)>,
) {
    if m.act.state.dmg_fly_all() {
        return;
    }
    m.reset_state(cx, OBJSTA_JUMP_ALL);
    if atk_flags & AF_FLYING != 0 {
        if m.act.state.bits() != OBJSTA_STUN {
            m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DMG_FLY);
            set_motion(m, cx, MTI_DMGFLY, ANILOOP_CONT, 0);
            if let Some((id, angle)) = attacker {
                cx.emit(SimEvent::AttackerStop { attacker: id });
                do_damage_fly(m, angle, 145.0, 0.18);
            }
        }
    } else {
        m.set_state(cx, OBJSTA_DMG_ALL, OBJSTA_DMG);
    }
    if m.skill.casting_end_frame > cx.frame() && m.skill.casting_skill == defs::SI_KNT_HERO_DRAWING
    {
        m.skill.casting_end_frame = 0;
        m.clear_state(cx);
    }
    let st = m.act.state;
    if st.is_act_jump() || st.is_act_attack() || st.is_action() {
        return;
    }
    m.motion.id = u32::MAX;
    if m.act.flags.fly() {
        if let Some(r) = m.params.ride {
            set_motion(
                m,
                cx,
                r.use_motion + MTA_FDMG1,
                ANILOOP_1PLAY,
                MOP_FIXED | MOP_NO_TRANS,
            );
        }
    } else {
        let pick = random_below(cx.rng, 2);
        set_motion(m, cx, MTI_DMG1 + pick, ANILOOP_1PLAY, 0);
    }
}

fn random_below(rng: &mut dyn GameRng, n: u32) -> u32 {
    rng.random_below(n)
}

/// `OBJMSG_DIE` (Plan/31 §6.2): clears every state, stops the horizontal velocity and enters `DEAD`, or
/// `DMG_FLY` with an extra knock-back when the cause was `OBJMSG_DAMAGE_FORCE`. Server NPCs start the
/// 180-frame dead counter.
pub fn die(m: &mut SimMover, cx: &mut Cx<'_>, cause: ObjMsg, attacker: Option<(ObjId, f32)>) {
    super::msg_ground::die_msg(m, cx, cause, attacker);
}

/// Convenience: sends `OBJMSG_DAMAGE`-style messages is doc 17's job; this builds the plain `DIE` message.
pub fn die_msg_for(cause: ObjMsg) -> ActMsg {
    ActMsg::with(OBJMSG_DIE, [cause as i32, 0, 0, 0, 0])
}
