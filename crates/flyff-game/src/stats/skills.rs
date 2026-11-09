//! Skill points: the skill reset, the master skill growth and the legend skill-up (Plan/16 §12.3, §12.4;
//! WP-MOVER-19; F-129, F-130). The grants and the caps are `progression::add_skill_points` (WP-MOVER-08); the
//! plan validation and the `DoUseSkillPointReq` handler are doc 17's (`validate_skill_plan`, WP-SKILL-10).
//!
//! Every function works on one mover and queues its messages in an [`Out`], so the item router (the skill reset
//! scroll), the GM commands and the level-up sequence use the same code.

use super::env::StatsCore;
use super::out::Out;
use crate::mover::Mover;
use flyff_common::defs::obj::XI_SYS_EXCHAN01;
use flyff_proto::pb;
use flyff_rules::consts::job::{JTYPE_HERO, JTYPE_MASTER};
use flyff_rules::jobs::SkillTable;
use flyff_rules::skill_points::{
    LegendUpRefusal, legend_up_check as rules_legend_up_check, master_skill_up_applies,
    reset_refund,
};

/// `DoUseSkillPointNtf{table, skill_points}` to the owner (`AddDoUseSkillPoint`): the whole server table, 45
/// positional slots, and the unspent points.
pub fn do_use_skill_point_ntf(skills: &SkillTable, skill_points: i32) -> pb::DoUseSkillPointNtf {
    pb::DoUseSkillPointNtf {
        skills: skills
            .iter()
            .map(|s| pb::Skill {
                skill_id: s.skill_id,
                level: s.level,
            })
            .collect(),
        skill_point: skill_points,
    }
}

/// `CMover::InitSkillExp()` (`Mover.cpp:1904-1949`, 16 §12.3), the skill reset (`II_SYS_SYS_SCR_RECCURENCE`,
/// `/InitSkillExp`): every skill that is neither a master nor a hero skill falls to level 0 and gives back
/// `level * cost` points (`flyff_rules::skill_points::reset_refund`); the unspent points are clamped to the cap
/// of the job line. The skill shortcuts leave the taskbar (the whole `TaskBar` is sent) and `InitSkillPoint{points}`
/// follows. `false` and nothing changed when a learned slot has no skill prop (the scroll is not consumed) or `m`
/// is not a player.
pub fn init_skill_exp(m: &mut Mover, c: &StatsCore, out: &mut Out) -> bool {
    let job = m.mv.job;
    let Some(p) = m.player_mut() else {
        return false;
    };
    let Some(r) = reset_refund(
        &p.progression.job_skills,
        &c.data.skills,
        &c.rules,
        p.progression.skill_points,
        job,
    ) else {
        return false;
    };
    p.progression.job_skills = r.skills;
    p.progression.skill_points = r.skill_points;
    crate::taskbar::reset_skill_shortcuts(&mut p.taskbar);
    let points = p.progression.skill_points;
    if let Some(tb) = crate::taskbar::player_taskbar_msg(m) {
        out.to_self(tb);
    }
    out.to_self(pb::InitSkillPoint {
        skill_point: points,
    });
    true
}

/// Adds one level to every skill whose prop is of the tier `kind` (`JTYPE_MASTER` or `JTYPE_HERO`) and queues the
/// effect near the player and the new table to the owner. Returns how many skills grew.
fn grow_tier(m: &mut Mover, c: &StatsCore, kind: u32, out: &mut Out) -> usize {
    let pos = m.core.pos;
    let Some(p) = m.player_mut() else { return 0 };
    let mut grown = 0;
    for slot in p
        .progression
        .job_skills
        .iter_mut()
        .filter(|s| !s.is_empty())
    {
        if c.data
            .skills
            .get(slot.skill_id)
            .is_some_and(|prop| prop.item_kind1 == kind)
        {
            slot.level = slot.level.saturating_add(1);
            grown += 1;
        }
    }
    out.near(pb::CreateSfxObj {
        sfx_id: XI_SYS_EXCHAN01,
        x: pos.x,
        y: pos.y,
        z: pos.z,
        flag: false,
    });
    out.to_self(do_use_skill_point_ntf(
        &p.progression.job_skills,
        p.progression.skill_points,
    ));
    grown
}

/// `CMover::SetMasterSkillPointUp()` (`MoverSkill.cpp:64-93`, 16 §12.4) for the level-up to `level`: a **master**
/// reaching 72, 84, 96 or 108 gets +1 on every master skill (heroes and everybody else nothing), the effect
/// `XI_SYS_EXCHAN01` near and `DoUseSkillPointNtf` to the owner. Returns `true` when it applied.
pub fn master_skill_up(m: &mut Mover, c: &StatsCore, level: i32, out: &mut Out) -> bool {
    if !master_skill_up_applies(&c.rules, m.mv.job, level) {
        return false;
    }
    grow_tier(m, c, JTYPE_MASTER, out);
    true
}

/// The checks of `LegendSkillUpStart` before the gems (`DPSrvr.cpp:10519-10538`): a hero whose hero skills are all
/// below level 5. `Err(NotHero)` is answered with nothing, `Err(AtMaximum)` with `LegendSkillUpResult{-1}`.
pub fn legend_up_check(m: &Mover, c: &StatsCore) -> Result<(), LegendUpRefusal> {
    let Some(p) = m.player() else {
        return Err(LegendUpRefusal::NotHero);
    };
    rules_legend_up_check(
        &c.rules,
        m.mv.job,
        &p.progression.job_skills,
        &c.data.skills,
    )
}

/// The success of the legend skill-up (`DPSrvr.cpp:10571-10592`): every hero skill +1, the effect
/// `XI_SYS_EXCHAN01` near and `DoUseSkillPointNtf` to the owner. The caller consumed the five gems, rolled with
/// `legend_up_roll` and sends `LegendSkillUpResult{1}`.
pub fn legend_skill_up(m: &mut Mover, c: &StatsCore, out: &mut Out) {
    grow_tier(m, c, JTYPE_HERO, out);
}
