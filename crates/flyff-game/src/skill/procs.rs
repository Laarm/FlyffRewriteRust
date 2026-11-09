//! Procs and the skill-engine side of the damage pipeline's seams (Plan/17 §5.15, §6.11, §5.14; WP-SKILL-08;
//! `src:WORLDSERVER/AttackArbiter.cpp:520-660`, `Ctrl.cpp:1683-1707`).
//!
//! The damage pipeline decides **whether** a proc fires (it owns the rolls, in the order of the original, so the
//! draws line up): [`ChanceSkill`](crate::combat::arbiter) for the chance DSTs of the attacker
//! (`DST_CHR_CHANCESTUN / POISON / DARK / BLEEDING / STEALHP`: the attacker holds an add-skill row id in the
//! DST's `chg` slot, the proc is the row's skill and level), the weapon's `active_skill` (`ignore_prob`: the
//! weapon rolled its own `active_skill_rate`), and the level row's `active_skill`. Each proc is a
//! [`ProcHook::do_active_skill`](crate::combat::ProcHook), whose default body queues a
//! [`SkillJob::Active`](super::cast::SkillJob); the jobs run here, in the service context, right after the damage
//! call ([`super::cast::drain_jobs`]) or after the world phase.
//!
//! * [`run_active`]: `DoActiveSkill(skill, level, target, ignore_prob)` of the attacker - a shot-type skill has no
//!   server effect (KQ-07), any other is applied through [`do_apply_skill`](super::area::do_apply_skill) and
//!   announced with `DoApplyUseSkill`.
//! * [`run_counter`]: the counter-attack skill of a victim in `CHS_ATK_COUNTERATTACK` /
//!   `CHS_DMG_COUNTERATTACK` (`DoUseSkill(SI_GEN_ATK_COUNTER, level, attacker, SUT_NORMAL, false, 0)`; the skill
//!   start skips no cast motion for it).
//! * [`remove_invisible`]: `RemoveInvisible()`: a mover that attacks or is processed as an attacker loses its
//!   invisibility and the `SI_ACR_SUP_DARKILLUSION` buff.
//!
//! The buff removals of the seams (`remove_skill_buff`, `remove_chr_state_buffs`) are
//! [`buff::api::remove_skill`](crate::buff::api::remove_skill) and
//! [`buff::api::remove_flags`](crate::buff::api::remove_flags) with `RemoveFlags::CHRSTATE`.

use super::use_skill::{SkillUseType, use_skill_direct};
use crate::buff::api::with_mover;
use crate::buff::core::remove_fake;
use crate::buff::set::{BuffKey, BuffKind};
use crate::buff::status;
use crate::combat::ActiveSkill;
use crate::ctx::{GameCtx, Services};
use flyff_common::ObjId;
use flyff_common::defs::skill::SI_ACR_SUP_DARKILLUSION;

/// `DoActiveSkill(skill, level, target, ignore_prob)` of the proc's caster.
pub fn run_active(svc: &mut Services, ctx: &mut GameCtx<'_>, a: &ActiveSkill) {
    if ctx.worlds.mover(a.caster).is_none() || ctx.worlds.mover(a.target).is_none() {
        return;
    }
    super::apply::do_active_skill(
        svc,
        ctx,
        a.caster,
        a.skill,
        a.level,
        a.target,
        a.ignore_prob,
    );
}

/// The counter-attack cast of `caster` at `target` (see the module documentation). A refusal of the start is
/// the original's `FALSE` return and changes nothing.
pub fn run_counter(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    caster: ObjId,
    skill: u32,
    level: u32,
    target: ObjId,
) {
    let _ = crate::combat::with_action(svc, ctx, caster, |cx| {
        use_skill_direct(
            cx,
            caster,
            skill,
            level,
            target,
            SkillUseType::Normal,
            false,
            0,
        )
    });
}

/// `CMover::RemoveInvisible()`: the invisibility state and the Dark Illusion buff of `mover` go.
pub fn remove_invisible(svc: &mut Services, ctx: &mut GameCtx<'_>, mover: ObjId) {
    with_mover(svc, ctx, mover, |m, cx, fx| {
        status::clear_dark_cover(m, cx, fx);
        remove_fake(
            m,
            cx,
            BuffKey::new(BuffKind::Skill, SI_ACR_SUP_DARKILLUSION),
            fx,
        );
    });
}
