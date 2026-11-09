//! The skill-plan rule (VR-086; Plan/16 §12.2, Plan/17 §6.2, WP-MOVER-24 rule part): is a desired table of the 45
//! job-skill slots a legal way to spend skill points?
//!
//! The original server (`OnDoUseSkillPoint`, `DPSrvr.cpp:3299-3362`) only checked `level >= current`,
//! `level <= expert_max` and the point sum, and it priced a level with the skill id **from the packet**. The
//! character level, the prerequisites and `nLog` were enforced by the client window alone (`CMover::CheckSkill`,
//! `MoverParam.cpp:117-155`, evaluated on the window's local copy). [`validate_skill_plan`] is the one function that
//! enforces all of it on the server's own table; the browser's skill-tree window runs the same function.

use crate::consts::NULL_ID;
use crate::data::RulesData;
use crate::jobs::{SkillTable, is_hero, is_master};
use crate::skill_points::sp_cost;
use flyff_data::props::item::SkillProp;
use flyff_data::types::IdTable;

/// Why a plan is refused ([`validate_skill_plan`]). Every variant but [`SkillPlanError::NothingToDo`] is answered
/// with `DefinedText{TID_RESKILLPOINT_ERROR}`; `NothingToDo` is ignored without an answer (as the original did for
/// a zero cost).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillPlanError {
    /// The plan names another skill than the server's slot `slot` holds.
    WrongSkill {
        /// The slot (0..45).
        slot: u8,
    },
    /// The planned level is below the current one (points are never taken back; only the reset item does).
    LevelDown {
        /// The slot.
        slot: u8,
    },
    /// The planned level is above `expert_max` of the skill.
    AboveMax {
        /// The slot.
        slot: u8,
    },
    /// The character level is below the skill's `req_dis_lv` (masters and heroes are exempt).
    ReqLevel {
        /// The slot.
        slot: u8,
    },
    /// A prerequisite skill of the plan is below the level the skill needs, or the skill cannot be learned at all
    /// (`nLog == 1`).
    Prerequisite {
        /// The slot.
        slot: u8,
    },
    /// The plan costs more points than the player has.
    NotEnoughPoints,
    /// The plan changes nothing (cost 0): ignored.
    NothingToDo,
}

impl SkillPlanError {
    /// The numeric code of the browser export `skill_plan_check` (`-(code + 1)`): 0 wrong skill, 1 level down, 2 above
    /// max, 3 level requirement, 4 prerequisite, 5 not enough points, 6 nothing to do.
    pub fn code(self) -> u8 {
        match self {
            SkillPlanError::WrongSkill { .. } => 0,
            SkillPlanError::LevelDown { .. } => 1,
            SkillPlanError::AboveMax { .. } => 2,
            SkillPlanError::ReqLevel { .. } => 3,
            SkillPlanError::Prerequisite { .. } => 4,
            SkillPlanError::NotEnoughPoints => 5,
            SkillPlanError::NothingToDo => 6,
        }
    }

    /// The slot the error is about; `None` for the two whole-plan errors.
    pub fn slot(self) -> Option<u8> {
        match self {
            SkillPlanError::WrongSkill { slot }
            | SkillPlanError::LevelDown { slot }
            | SkillPlanError::AboveMax { slot }
            | SkillPlanError::ReqLevel { slot }
            | SkillPlanError::Prerequisite { slot } => Some(slot),
            SkillPlanError::NotEnoughPoints | SkillPlanError::NothingToDo => None,
        }
    }

    /// `true` when the player is told `TID_RESKILLPOINT_ERROR` (every error but `NothingToDo`).
    pub fn is_reported(self) -> bool {
        self != SkillPlanError::NothingToDo
    }
}

/// VR-086: validates `plan` against the server's table `cur` for a character of `level` and `job` with `sp`
/// unspent skill points and returns the cost in points.
///
/// For every slot of `cur` that holds a skill:
///
/// 1. `plan` must hold the same skill id ([`SkillPlanError::WrongSkill`]); a skill the data does not know is only
///    accepted unchanged;
/// 2. `plan.level >= cur.level` ([`SkillPlanError::LevelDown`]) and `plan.level <= expert_max`
///    ([`SkillPlanError::AboveMax`]);
/// 3. when the level rises: `nLog != 1` and, unless the character is a master or a hero, `level >= req_dis_lv`
///    ([`SkillPlanError::ReqLevel`]), and both prerequisites `re_skill1/2 >= re_skill_level1/2` **in the plan**
///    ([`SkillPlanError::Prerequisite`]; a prerequisite skill that is not in the table is ignored, as
///    `CheckSkill` does);
/// 4. the cost grows by `(plan.level - cur.level) * sp_cost`.
///
/// Slots the server holds empty are ignored. A cost of 0 is [`SkillPlanError::NothingToDo`]; a cost above `sp`
/// is [`SkillPlanError::NotEnoughPoints`]. Pure and deterministic (tier S): the same function runs in the browser.
pub fn validate_skill_plan(
    cur: &SkillTable,
    plan: &SkillTable,
    level: i32,
    job: i32,
    sp: i32,
    skills: &IdTable<SkillProp>,
    d: &RulesData,
) -> Result<i32, SkillPlanError> {
    let legend = is_master(d, job) || is_hero(d, job);
    let mut cost: i64 = 0;
    for (i, (c, p)) in cur.iter().zip(plan.iter()).enumerate() {
        if c.is_empty() {
            continue;
        }
        let slot = u8::try_from(i).unwrap_or(u8::MAX);
        if p.skill_id != c.skill_id {
            return Err(SkillPlanError::WrongSkill { slot });
        }
        let Some(prop) = skills.get(c.skill_id) else {
            // data without the skill: nothing can be priced, so only an unchanged slot is acceptable
            if p.level != c.level {
                return Err(SkillPlanError::WrongSkill { slot });
            }
            continue;
        };
        if p.level < c.level {
            return Err(SkillPlanError::LevelDown { slot });
        }
        if p.level > prop.expert_max {
            return Err(SkillPlanError::AboveMax { slot });
        }
        if p.level == c.level {
            continue;
        }
        // `CheckSkill`: a skill flagged `nLog == 1` cannot be learned
        if prop.log == 1 {
            return Err(SkillPlanError::Prerequisite { slot });
        }
        if prop.req_dis_lv != NULL_ID && !legend && i64::from(level) < i64::from(prop.req_dis_lv) {
            return Err(SkillPlanError::ReqLevel { slot });
        }
        for (need_skill, need_level) in [
            (prop.re_skill1, prop.re_skill_level1),
            (prop.re_skill2, prop.re_skill_level2),
        ] {
            if need_skill == NULL_ID {
                continue;
            }
            let have = plan
                .iter()
                .find(|s| !s.is_empty() && s.skill_id == need_skill);
            if have.is_some_and(|s| s.level < need_level) {
                return Err(SkillPlanError::Prerequisite { slot });
            }
        }
        cost += i64::from(p.level - c.level) * i64::from(sp_cost(prop, d));
    }
    if cost == 0 {
        return Err(SkillPlanError::NothingToDo);
    }
    if cost > i64::from(sp) {
        return Err(SkillPlanError::NotEnoughPoints);
    }
    // `cost <= sp <= i32::MAX`
    Ok(i32::try_from(cost).unwrap_or(i32::MAX))
}
