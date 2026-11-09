//! The seam of the skill hit handlers (Plan/17 §6.6, §4.4; owner WP-SKILL-05).
//!
//! The hit frames of the skill states (`AttrHit{MeleeSkill | MagicSkill}`, the repeated shots of a multi-shot
//! skill) and the impact of a skill projectile belong to the skill work packages. The action layer only
//! dispatches to this trait; the default does nothing, so a skill state ends without damage until WP-SKILL-05
//! installs its handlers in [`CombatWorld::skill`](super::world::CombatWorld::skill).
//!
//! The handlers get the whole [`ActionCx`]: they send damage through `cx.arb`, spawn projectiles through
//! `cx.q.projectiles` and use the sim through [`ActionCx::sim_call`].

use super::context::ActionCx;
use flyff_common::ObjId;
use flyff_sim::step::{HitKind, ShotKind};

/// A skill hit frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillHit {
    /// The caster.
    pub attacker: ObjId,
    /// [`HitKind::MeleeSkill`] or [`HitKind::MagicSkill`].
    pub kind: HitKind,
    /// The target of the state.
    pub target: ObjId,
    /// The hit index of a multi-hit skill.
    pub hit_index: i32,
    /// The state word at the hit.
    pub state: u32,
    /// The frame being stepped.
    pub frame: u64,
}

/// A repeated shot of a multi-shot skill (every 4th frame while `shoot_cnt > 0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillShot {
    /// The caster.
    pub attacker: ObjId,
    /// What repeats.
    pub kind: ShotKind,
    /// The frame being stepped.
    pub frame: u64,
}

/// A skill projectile arrived (`AttackBySFX` with `IsSkillAttack`): `do_apply_skill` of the caster's current
/// level (Plan/17 §4.4 step 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillImpact {
    /// The caster.
    pub attacker: ObjId,
    /// The target.
    pub target: ObjId,
    /// The skill id.
    pub skill: u32,
    /// The projectile's `SFXHIT_INFO.nMagicPower` / `dwAtkFlags`.
    pub magic_power: u32,
    /// The `AF_*` word of the info.
    pub atk_flags: u32,
    /// The control flag (`bControl`).
    pub control: bool,
    /// The caster's angle at the hit frame.
    pub dmg_angle: f32,
    /// The knock-back power (`dmg_shift * 0.2`; 0 = none).
    pub dmg_power: f32,
}

/// The skill hit handlers (see the module documentation).
pub trait SkillHitHooks: Send {
    /// `OnMeleeSkill` / `OnMagicSkill` (`MoverActEvent.cpp:72-164, 475-705`).
    fn on_skill_hit(&mut self, _cx: &mut ActionCx<'_>, _hit: &SkillHit) {}

    /// A repeated shot of a multi-shot skill.
    fn on_repeat_shot(&mut self, _cx: &mut ActionCx<'_>, _shot: &SkillShot) {}

    /// A skill projectile arrived at a live target; the handler applies the skill. Returns `true` when it did
    /// (the default does not, so the shot is a plain effect until the skill engine lands).
    fn on_skill_impact(&mut self, _cx: &mut ActionCx<'_>, _impact: &SkillImpact) -> bool {
        false
    }
}

/// The handlers that do nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSkillHits;

impl SkillHitHooks for NoSkillHits {}
