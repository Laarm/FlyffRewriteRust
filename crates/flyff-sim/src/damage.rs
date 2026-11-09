//! The damage entry points of the game (Plan/31 §16.2 `flyff_sim::damage`; WP-SIM-20).
//!
//! The damage RULES (hit rolls, numbers, `PreProcessDamage`) are doc 17's; the sim owns what a hit does to
//! the defender's state machine: [`set_damaged_motion`] (`CMover::SetDamagedMotion`), [`do_damage_fly`]
//! (`CActionMover::DoDamageFly`) and [`die`] (`OBJMSG_DIE`). These wrappers take the same arguments as
//! [`crate::send_act_msg`] (environment, rules, rng, event sink); the browser reaches them through the
//! `DAMAGE_FX`, `PUSH_POWER` and `DIE` commands. Everything they cause happens at the barrier of the hit
//! frame and is effective from the next frame (Plan/31 §3.4, §22 Q6), so a caller passes the environment of
//! the frame it applies the damage in.

use crate::mover::SimMover;
use crate::step::{Cx, SimEnv, SimEvents, SimRules};
use crate::types::ObjMsg;
use flyff_common::ObjId;
use flyff_common::rng::GameRng;

pub use crate::act::damage::do_damage_fly;

/// `CMover::SetDamagedMotion(attacker, atk_flags)` (`MoverAttack.cpp:2204-2256`, Plan/31 §8.6).
///
/// * a mover already in a knock-back state (`DMG_FLY_ALL`) is left alone;
/// * `AF_FLYING` hits enter `DMG_FLY` with `MTI_DMGFLY` unless the state word is exactly `STUN`
///   (KEEP-QUIRK), raise `AttackerStop` and push the defender away from `attacker` (`attacker` is the
///   attacker's id and yaw, `do_damage_fly(yaw, 145, 0.18)`); other hits enter `DMG`;
/// * a running `SI_KNT_HERO_DRAWING` cast (end frame in the future) is cancelled with `ClearState`;
/// * unless jumping, attacking or in an action state, the damage motion `MTI_DMG1 + random(2)` plays
///   (KEEP-QUIRK: it also overrides `MTI_DMGFLY` of a flying hit while the state stays `DMG_FLY`).
pub fn set_damaged_motion(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    atk_flags: u32,
    attacker: Option<(ObjId, f32)>,
    ev: &mut SimEvents,
) {
    let mut cx = Cx::new(env, rules, rng, ev);
    crate::act::damage::set_damaged_motion(m, &mut cx, atk_flags, attacker);
}

/// `OBJMSG_DIE` (Plan/31 §6.2): `cause` is `OBJMSG_DAMAGE` or `OBJMSG_DAMAGE_FORCE` (the latter flies the
/// corpse away from `attacker`); server NPCs start the 180-frame dead counter. Raises `RemoveInvisible` and
/// `Died{attacker}`.
pub fn die(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    cause: ObjMsg,
    attacker: Option<(ObjId, f32)>,
    ev: &mut SimEvents,
) {
    let mut cx = Cx::new(env, rules, rng, ev);
    crate::act::damage::die(m, &mut cx, cause, attacker);
}
