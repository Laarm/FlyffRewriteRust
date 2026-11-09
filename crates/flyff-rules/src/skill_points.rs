//! Skill-point grants and caps (Plan/16 §12.1; `CMover::AddSkillPoint`, `Mover.cpp:9477-9500`).
//!
//! WP-MOVER-08 provides the part the level-up needs ([`sp_cap`], [`grant`]); WP-MOVER-19 adds the cost of a skill
//! level ([`sp_cost`]), the cap of the reset ([`sp_cap_reset`]), the reset itself ([`reset_refund`]), the master
//! skill growth ([`master_skill_up_applies`]) and the legend skill roll ([`legend_up_roll`], [`legend_up_check`]).
//! The plan validation of WP-MOVER-24 is `validate_skill_plan`.

use crate::consts::job::{JTYPE_BASE, JTYPE_EXPERT, JTYPE_HERO, JTYPE_MASTER};
use crate::data::RulesData;
use crate::jobs::{JobSkill, SkillTable, is_hero, is_master, is_pro};
use flyff_common::rng::GameRng;
use flyff_data::props::item::SkillProp;
use flyff_data::types::IdTable;

/// VR-086 (WP-MOVER-24): the skill-plan rule lives in `skill/plan.rs`; it is re-exported here under its plan path.
pub use crate::skill::plan::{SkillPlanError, validate_skill_plan};

/// The generic cap of the skill points ever granted (`nMaxPoint = 1280`, "the Elementor's").
pub const SP_CAP_DEFAULT: i32 = 1280;
/// The cap of a Knight or Blade pro.
pub const SP_CAP_KNIGHT_BLADE: i32 = 870;
/// The cap of a Jester or Ranger pro.
pub const SP_CAP_JESTER_RANGER: i32 = 910;
/// The cap of a Ringmaster pro.
pub const SP_CAP_RINGMASTER: i32 = 902;
/// The cap of a Billposter or Psykeeper pro.
pub const SP_CAP_BILLPOSTER_PSYKEEPER: i32 = 950;

/// `AddSkillPoint`'s cap for `job`: 1280, and for **pros only** 870 (Knight, Blade), 910 (Jester, Ranger), 902
/// (Ringmaster) or 950 (Billposter, Psykeeper). Masters and heroes use the generic cap (their SP comes from the
/// master/hero growth, not from this path).
pub fn sp_cap(d: &RulesData, job: i32) -> i32 {
    use crate::consts::job::*;
    if !is_pro(d, job) {
        return SP_CAP_DEFAULT;
    }
    match job as u32 {
        JOB_KNIGHT | JOB_BLADE => SP_CAP_KNIGHT_BLADE,
        JOB_JESTER | JOB_RANGER => SP_CAP_JESTER_RANGER,
        JOB_RINGMASTER => SP_CAP_RINGMASTER,
        JOB_BILLPOSTER | JOB_PSYCHIKEEPER => SP_CAP_BILLPOSTER_PSYKEEPER,
        _ => SP_CAP_DEFAULT,
    }
}

/// The points `AddSkillPoint(n)` actually grants to a character that has been granted `total` so far
/// (`m_nSkillLevel`) under `cap`: nothing at or above the cap, otherwise `n` cut to the room that is left. The
/// caller adds the result to both `skill_points` and `skill_level_total`.
pub fn grant(total: i32, n: i32, cap: i32) -> i32 {
    if total >= cap {
        return 0;
    }
    if cap < total.wrapping_add(n) {
        return cap.wrapping_sub(total);
    }
    n
}

/// `CProject::GetSkillPoint(prop)` (`Project.cpp:5292`): the points one level of the skill costs, by the tier of
/// the skill (`item_kind1`): `vag_sp` for a vagrant skill, `expert_sp` for an expert skill, `pro_sp` for every
/// other (`Constant.inc`; 1 / 2 / 3 by default).
pub fn sp_cost(prop: &SkillProp, d: &RulesData) -> u32 {
    let [vag, expert, pro] = d.sp_cost;
    match prop.item_kind1 {
        JTYPE_BASE => vag,
        JTYPE_EXPERT => expert,
        _ => pro,
    }
}

/// The ceiling `InitSkillExp` clamps the refunded points to (`Mover.cpp:1930-1940`): 1280 by default; 870 for the
/// Knight and Blade lines, 910 for Jester and Ranger, 902 for Ringmaster, 950 for Billposter and Psykeeper. Unlike
/// [`sp_cap`] the line includes the master and the hero of the job (`pro + 10` and `pro + 18`).
pub fn sp_cap_reset(job: i32) -> i32 {
    use crate::consts::job::*;
    match u32::try_from(job) {
        Ok(
            JOB_KNIGHT | JOB_BLADE | JOB_KNIGHT_MASTER | JOB_BLADE_MASTER | JOB_KNIGHT_HERO
            | JOB_BLADE_HERO,
        ) => SP_CAP_KNIGHT_BLADE,
        Ok(
            JOB_JESTER | JOB_RANGER | JOB_JESTER_MASTER | JOB_RANGER_MASTER | JOB_JESTER_HERO
            | JOB_RANGER_HERO,
        ) => SP_CAP_JESTER_RANGER,
        Ok(JOB_RINGMASTER | JOB_RINGMASTER_MASTER | JOB_RINGMASTER_HERO) => SP_CAP_RINGMASTER,
        Ok(
            JOB_BILLPOSTER
            | JOB_PSYCHIKEEPER
            | JOB_BILLPOSTER_MASTER
            | JOB_PSYCHIKEEPER_MASTER
            | JOB_BILLPOSTER_HERO
            | JOB_PSYCHIKEEPER_HERO,
        ) => SP_CAP_BILLPOSTER_PSYKEEPER,
        _ => SP_CAP_DEFAULT,
    }
}

/// The result of a skill reset ([`reset_refund`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetRefund {
    /// The points handed back before the clamp (`sum of level * cost` over the reset skills).
    pub refund: i32,
    /// The skill table with every non-master, non-hero skill at level 0.
    pub skills: SkillTable,
    /// The unspent skill points afterwards: the old points plus the refund, clamped to [`sp_cap_reset`].
    pub skill_points: i32,
}

/// `CMover::InitSkillExp()` (`Mover.cpp:1904-1949`, 16 §12.3): every skill with `level > 0` that is neither a master
/// nor a hero skill (by the tier of its prop) gives back `level * cost` points and falls to level 0; the sum is
/// added to `skill_points` and the total clamped to the job line's cap ([`sp_cap_reset`]). `None` when a learned
/// slot names a skill the data does not have (the legacy function returned `FALSE` after having changed the
/// slots before it: the pure version changes nothing).
pub fn reset_refund(
    cur: &SkillTable,
    skills: &IdTable<SkillProp>,
    d: &RulesData,
    skill_points: i32,
    job: i32,
) -> Option<ResetRefund> {
    let mut table = *cur;
    let mut sp = skill_points;
    let mut refund = 0_i32;
    for slot in table.iter_mut().filter(|s: &&mut JobSkill| !s.is_empty()) {
        let prop = skills.get(slot.skill_id)?;
        if slot.level > 0 && prop.item_kind1 != JTYPE_MASTER && prop.item_kind1 != JTYPE_HERO {
            let back = (slot.level as i32).wrapping_mul(sp_cost(prop, d) as i32);
            refund = refund.wrapping_add(back);
            sp = sp.wrapping_add(back);
            slot.level = 0;
        }
    }
    Some(ResetRefund {
        refund,
        skills: table,
        skill_points: sp.min(sp_cap_reset(job)),
    })
}

/// The levels at which a **master** earns +1 on every master skill (`SetMasterSkillPointUp`,
/// `MoverSkill.cpp:64`): 72, 84, 96, 108. Heroes never do.
pub fn master_skill_up_applies(d: &RulesData, job: i32, level: i32) -> bool {
    matches!(level, 72 | 84 | 96 | 108) && !is_hero(d, job) && is_master(d, job)
}

/// The success threshold of the legend skill-up: `xRandom(1000) > 766`, 23.3 % (`DPSrvr.cpp:10512-10580`,
/// PL-013 scale).
pub const LEGEND_UP_ROLL_ABOVE: u32 = 766;

/// The legend skill-up roll: `rng.random_below(1000) > 766`.
pub fn legend_up_roll(rng: &mut dyn GameRng) -> bool {
    rng.random_below(1000) > LEGEND_UP_ROLL_ABOVE
}

/// Why `LegendSkillUpStart` does nothing ([`legend_up_check`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendUpRefusal {
    /// The player is not a hero: the handler returns without an answer (`DPSrvr.cpp:10519`).
    NotHero,
    /// A hero skill is already above level 4: `LegendSkillUpResult{-1}`.
    AtMaximum,
}

/// The checks of `LegendSkillUpStart` before the gems are looked at (`DPSrvr.cpp:10519-10538`): the player must be
/// a hero, and no `JTYPE_HERO` skill may be above level 4. `Ok` means the five gems may be checked and consumed.
pub fn legend_up_check(
    d: &RulesData,
    job: i32,
    cur: &SkillTable,
    skills: &IdTable<SkillProp>,
) -> Result<(), LegendUpRefusal> {
    if !is_hero(d, job) {
        return Err(LegendUpRefusal::NotHero);
    }
    let maxed = cur.iter().filter(|s| !s.is_empty()).any(|s| {
        s.level > 4
            && skills
                .get(s.skill_id)
                .is_some_and(|p| p.item_kind1 == JTYPE_HERO)
    });
    if maxed {
        Err(LegendUpRefusal::AtMaximum)
    } else {
        Ok(())
    }
}
