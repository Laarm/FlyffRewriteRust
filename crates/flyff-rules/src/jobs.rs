//! Job predicates (Plan/16 §11.1, §13.1; `MoverParam.cpp:372-460`).
//!
//! The tier of a job comes from the job table (`etc.inc`, `JobRow::job_type`), exactly like the legacy
//! `IsBaseJob/IsExpert/IsPro/IsMaster/IsHero`; the id arithmetic helpers (`expert_of`, `pro_of`, `master_of`)
//! encode the fixed layout of `defineJob.h` (experts 1-5, pros 6-15, masters 16-23, heroes 24-31). A job of
//! `-1` (`JOB_NONE`, every NPC) or outside the table is of no tier: every predicate answers `false`.

use crate::consts::job::{
    JOB_VAGRANT, JTYPE_BASE, JTYPE_EXPERT, JTYPE_HERO, JTYPE_MASTER, JTYPE_PRO, MAX_EXPERT,
    MAX_HERO, MAX_JOBBASE, MAX_MASTER, MAX_PROFESSIONAL,
};
use crate::consts::{
    MAX_EXP_LEVEL, MAX_EXPCHARACTER, MAX_JOB_LEVEL, MAX_LEVEL, MAX_SKILL_JOB, NULL_ID,
    SKILL_BLOCK_START,
};
use crate::data::RulesData;
use flyff_data::props::item::SkillProp;
use flyff_data::props::skill::JobSkills;
use flyff_data::types::IdTable;
use smallvec::SmallVec;

/// "No tier": the result of [`job_type`] for `-1` or an unknown job.
pub const JOB_TYPE_NONE: u32 = u32::MAX;

/// The `dwJobType` of `job` (`prj.m_aJob[job].dwJobType`); [`JOB_TYPE_NONE`] for `-1` or a job the table does
/// not list.
pub fn job_type(d: &RulesData, job: i32) -> u32 {
    d.job(job).map_or(JOB_TYPE_NONE, |r| r.job_type)
}

/// `IsBaseJob()`: the vagrant.
pub fn is_base(d: &RulesData, job: i32) -> bool {
    job_type(d, job) == JTYPE_BASE
}

/// `IsExpert()`.
pub fn is_expert(d: &RulesData, job: i32) -> bool {
    job_type(d, job) == JTYPE_EXPERT
}

/// `IsPro()`.
pub fn is_pro(d: &RulesData, job: i32) -> bool {
    job_type(d, job) == JTYPE_PRO
}

/// `IsMaster()` (`dwJobType == 5`).
pub fn is_master(d: &RulesData, job: i32) -> bool {
    job_type(d, job) == JTYPE_MASTER
}

/// `IsHero()` (`dwJobType == 6`).
pub fn is_hero(d: &RulesData, job: i32) -> bool {
    job_type(d, job) == JTYPE_HERO
}

/// `GetLegendChar()`: 0 normal, 1 master, 2 hero.
pub fn legend_class(d: &RulesData, job: i32) -> u8 {
    if is_master(d, job) {
        1
    } else if is_hero(d, job) {
        2
    } else {
        0
    }
}

/// The expert a pro advances from: `expert * 2 + 4` and `expert * 2 + 5` are its pros (`None` outside 6..16).
pub fn expert_of(pro: i32) -> Option<i32> {
    (MAX_EXPERT as i32..MAX_PROFESSIONAL as i32)
        .contains(&pro)
        .then(|| (pro - 4) / 2)
}

/// The pro a master is the legend form of: `master - 10` (`None` outside 16..24).
pub fn pro_of(master: i32) -> Option<i32> {
    (MAX_PROFESSIONAL as i32..MAX_MASTER as i32)
        .contains(&master)
        .then(|| master - 10)
}

/// The master a hero is the legend form of: `hero - 8` (`None` outside 24..32).
pub fn master_of(hero: i32) -> Option<i32> {
    (MAX_MASTER as i32..crate::consts::MAX_JOB as i32)
        .contains(&hero)
        .then(|| hero - 8)
}

/// `CMover::IsInteriorityJob(item_job)` (`MoverParam.cpp:408-460`): may a mover of job `own` use an item
/// restricted to `item_job`? The vagrant and the own job always; a pro may use its expert's items; a master
/// those of its expert (`j * 2 + 14/15`) and pro (`j + 10`); a hero those of its expert (`j * 2 + 22/23`),
/// pro (`j + 18`) and master (`j + 8`, `__NEW_ITEM_LIMIT_LEVEL`).
pub fn is_interiority_job(d: &RulesData, own: i32, item_job: i32) -> bool {
    if item_job == JOB_VAGRANT as i32 || item_job == own {
        return true;
    }
    let (expert_end, pro_end, master_end) = (
        MAX_EXPERT as i32,
        MAX_PROFESSIONAL as i32,
        MAX_MASTER as i32,
    );
    // an expert's pros are `j * 2 + 4` and `j * 2 + 5`; its masters `j * 2 + 14/15`; its heroes `j * 2 + 22/23`
    let expert_line = |first: i32| {
        item_job < expert_end && (own == item_job * 2 + first || own == item_job * 2 + first + 1)
    };
    if is_pro(d, own) && 0 < item_job && expert_line(4) {
        return true;
    }
    if is_master(d, own) {
        if expert_line(14) {
            return true;
        }
        if (expert_end..pro_end).contains(&item_job) && item_job + 10 == own {
            return true;
        }
    }
    if is_hero(d, own) {
        if expert_line(22) {
            return true;
        }
        if (expert_end..pro_end).contains(&item_job) && item_job + 18 == own {
            return true;
        }
        if (pro_end..master_end).contains(&item_job) && item_job + 8 == own {
            return true;
        }
    }
    false
}

/// The tier of `job` by the fixed `defineJob.h` layout (0 base, 1 expert, 2 pro, 5 master, 6 hero;
/// [`JOB_TYPE_NONE`] outside 0..32). The data-driven [`job_type`] must agree with it for the shipped data.
pub fn job_type_by_layout(job: i32) -> u32 {
    match job {
        0 => JTYPE_BASE,
        1..=5 => JTYPE_EXPERT,
        6..=15 => JTYPE_PRO,
        16..=23 => JTYPE_MASTER,
        24..=31 => JTYPE_HERO,
        _ => JOB_TYPE_NONE,
    }
}

// ---- job skill slots and the AddChangeJob fill (16 §11.2) ---------------------------------------------------------

/// One slot of `m_aJobSkill` (`SKILL`, 8 bytes): the skill id ([`NULL_ID`] = empty slot) and its level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobSkill {
    /// The skill id; [`NULL_ID`] = empty slot.
    pub skill_id: u32,
    /// The skill level.
    pub level: u32,
}

impl JobSkill {
    /// An unlearned slot (`{NULL_ID, 0}`).
    pub const EMPTY: JobSkill = JobSkill {
        skill_id: NULL_ID,
        level: 0,
    };

    /// `true` for an empty slot.
    pub fn is_empty(&self) -> bool {
        self.skill_id == NULL_ID
    }
}

/// The 45 skill slots of a player (`m_aJobSkill[MAX_SKILL_JOB]`): vagrant 0..3, expert 3..23, pro 23..43, master
/// 43, hero 44.
pub type SkillTable = [JobSkill; MAX_SKILL_JOB];

/// A table with every slot empty.
pub const EMPTY_SKILLS: SkillTable = [JobSkill::EMPTY; MAX_SKILL_JOB];

/// The skill data the layout functions read: `GameData.skills` and the per-job lists of `LoadSkill`.
#[derive(Clone, Copy)]
pub struct SkillSource<'a> {
    /// `propSkill.txt` by skill id.
    pub skills: &'a IdTable<SkillProp>,
    /// `prj.m_aJobSkill`: the skill ids of each job ordered by the required level (PL-016).
    pub job_skills: &'a JobSkills,
}

/// The first slot of the skill block of `job` (`AddChangeJob`): experts 1..=5 start at 3, pros 6..=15 at 23,
/// masters 16..=23 at 43, heroes 24..=31 at 44. `None` for the vagrant, `-1` and every other id (the legacy
/// `AddChangeJob` does nothing for them).
pub fn skill_slot_base(job: i32) -> Option<usize> {
    let j = u32::try_from(job).ok()?;
    let [_, expert, pro, master, hero] = SKILL_BLOCK_START;
    if (MAX_JOBBASE..MAX_EXPERT).contains(&j) {
        Some(expert)
    } else if (MAX_EXPERT..MAX_PROFESSIONAL).contains(&j) {
        Some(pro)
    } else if (MAX_PROFESSIONAL..MAX_MASTER).contains(&j) {
        Some(master)
    } else if (MAX_MASTER..MAX_HERO).contains(&j) {
        Some(hero)
    } else {
        None
    }
}

/// The level a freshly granted skill of `skill_id` gets in the tier block of `job`: `expert_max` with
/// `features.auto_skill_max`, else 0 (masters 1: "master skills start at level 1").
fn fresh_level(src: &SkillSource<'_>, skill_id: u32, job: i32, auto_max: bool) -> u32 {
    if auto_max {
        src.skills.get(skill_id).map_or(0, |p| p.expert_max)
    } else if (MAX_PROFESSIONAL as i32..MAX_MASTER as i32).contains(&job) {
        1
    } else {
        0
    }
}

/// The skill fill of `CMover::AddChangeJob(job)` (`MoverParam.cpp:1599-1688`) on `table`: the skills of `job` go
/// into the block of its tier, in list order, at `fresh_level`; slots outside the block are untouched. Returns
/// `false` (and changes nothing) for a job `AddChangeJob` rejects: the vagrant, `-1`, an id above 31. A skill
/// list longer than the block spills into the next block like the original (bounded by the table).
pub fn fill_job_block(
    table: &mut SkillTable,
    src: &SkillSource<'_>,
    job: i32,
    auto_max: bool,
) -> bool {
    let Some(base) = skill_slot_base(job) else {
        return false;
    };
    for (i, &id) in src.job_skills.of_job(job as u32).iter().enumerate() {
        if let Some(slot) = table.get_mut(base + i) {
            slot.skill_id = id;
            slot.level = fresh_level(src, id, job, auto_max);
        }
    }
    true
}

/// The master skill level for a master of `level` (`InitLevel`'s overlapping chain, first match wins): 59..=72
/// give 1, 71..=84 give 2, 83..=96 give 3, 95..=108 give 4, 107..=120 give 5, anything else 1.
pub fn master_skill_level_for(level: i32) -> u32 {
    match level {
        59..=72 => 1,
        73..=84 => 2,
        85..=96 => 3,
        97..=108 => 4,
        109..=120 => 5,
        _ => 1,
    }
}

/// The level of the hero skill table `InitLevel` gives every master skill of a hero.
pub const HERO_MASTER_SKILL_LEVEL: u32 = 5;

/// What `CMover::InitLevel(job, level)` builds (Plan/16 §11.3), ready to be copied into a player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitLevelPlan {
    /// The job the player ends with (`m_nJob`): the last `AddChangeJob` that took effect, [`JOB_VAGRANT`] when
    /// `level` never reached 15.
    pub job: i32,
    /// The final level (`level` clamped to `1..=199`).
    pub level: i32,
    /// `m_nRemainGP`.
    pub stat_points: i32,
    /// `m_nSkillPoint` = `m_nSkillLevel`.
    pub skill_points: i32,
    /// `m_nDeathLevel` (= the level).
    pub death_level: i32,
    /// The 45 skill slots.
    pub skills: SkillTable,
}

/// The pure part of `CMover::InitLevel(job, level)` (`Mover.cpp:1598-1873`, 16 §11.3) for a player: the vagrant
/// skills, the stat points of the levels `2..=level` (`lp_point` + 1 per level above 60 for jobs of the master
/// and hero tiers + 12 at level 121), the `AddChangeJob` chain at levels 15 and 60, the skill points of the
/// final level and job ([`sp_init_for`]), and the master / hero skill levels. `level` is clamped to the rows of
/// the EXP table (`1..=199`).
///
/// The legacy code sent the *requested* job to the client even when the loop never reached the level of the
/// change; the plan carries the job the player really has.
pub fn init_level_plan(
    d: &RulesData,
    src: &SkillSource<'_>,
    job: i32,
    level: i32,
    auto_max: bool,
) -> InitLevelPlan {
    let level = level.clamp(1, MAX_EXPCHARACTER as i32 - 1);
    let mut skills = EMPTY_SKILLS;
    // 1. the vagrant skills come first
    for (i, &id) in src.job_skills.of_job(JOB_VAGRANT).iter().enumerate() {
        if let Some(slot) = skills.get_mut(i) {
            *slot = JobSkill {
                skill_id: id,
                level: if auto_max {
                    src.skills.get(id).map_or(0, |p| p.expert_max)
                } else {
                    0
                },
            };
        }
    }
    let mut cur_job = JOB_VAGRANT as i32;
    let add = |skills: &mut SkillTable, cur: &mut i32, j: i32| {
        if fill_job_block(skills, src, j, auto_max) {
            *cur = j;
        }
    };
    // 3. GP and the job chain
    let mut gp = 0_i32;
    let (expert_end, pro_end, master_end) = (
        MAX_EXPERT as i32,
        MAX_PROFESSIONAL as i32,
        MAX_MASTER as i32,
    );
    for i in 1..level {
        let lvl = i + 1;
        gp = gp.wrapping_add(d.exp_row(lvl).map_or(0, |r| r.lp_point as i32));
        if job >= pro_end && i > 59 {
            gp = gp.wrapping_add(1);
        }
        if i == MAX_LEVEL {
            gp = gp.wrapping_add(12);
        }
        if lvl == MAX_JOB_LEVEL {
            let first = if job < pro_end {
                if job < expert_end {
                    job
                } else if job % 2 != 0 {
                    (job - 5) / 2
                } else {
                    (job - 4) / 2
                }
            } else if job < master_end {
                if job % 2 != 0 {
                    (job - 15) / 2
                } else {
                    (job - 14) / 2
                }
            } else if job % 2 != 0 {
                (job - 23) / 2
            } else {
                (job - 22) / 2
            };
            add(&mut skills, &mut cur_job, first);
        } else if lvl == MAX_JOB_LEVEL + MAX_EXP_LEVEL {
            if job < pro_end {
                add(&mut skills, &mut cur_job, job);
            } else if job < master_end {
                add(&mut skills, &mut cur_job, job - 10);
                add(&mut skills, &mut cur_job, job);
            } else {
                add(&mut skills, &mut cur_job, job - 18);
                add(&mut skills, &mut cur_job, job - 8);
                add(&mut skills, &mut cur_job, job);
            }
        }
    }
    // 4. the skill points of the final level and job
    let sp = sp_init_for(level, cur_job);
    // 5./6. the master and hero skills
    let master = is_master(d, cur_job);
    let hero = is_hero(d, cur_job);
    if master || hero {
        let lvl = if master {
            master_skill_level_for(level)
        } else {
            HERO_MASTER_SKILL_LEVEL
        };
        for slot in skills.iter_mut().filter(|s| !s.is_empty()) {
            if src
                .skills
                .get(slot.skill_id)
                .is_some_and(|p| p.item_kind1 == JTYPE_MASTER)
            {
                slot.level = lvl;
            }
        }
    }
    InitLevelPlan {
        job: cur_job,
        level,
        stat_points: gp,
        skill_points: sp,
        death_level: level,
        skills,
    }
}

/// [`init_level_plan`]'s skill slots only (the plan's `init_level_skill_layout`).
pub fn init_level_skill_layout(
    d: &RulesData,
    src: &SkillSource<'_>,
    job: i32,
    level: i32,
    auto_max: bool,
) -> SkillTable {
    init_level_plan(d, src, job, level, auto_max).skills
}

/// The skill points `InitLevel` grants for a character of `level` and `job` (`Mover.cpp:1763-1789`): the
/// piecewise line of the level (`2L-2`, `3L-22`, `4L-62`, `5L-122`, `6L-202`, `7L-302` up to 20, 40, 60, 80, 100,
/// beyond) plus the bonus of the final job (Mercenary 40, Acrobat 50, Assist 60, Magician 90, Knight and Blade
/// 120, Jester and Ranger 150, Ringmaster 160, Billposter and Psykeeper 180, Elementor 390; masters and heroes
/// none).
pub fn sp_init_for(level: i32, job: i32) -> i32 {
    use crate::consts::job::*;
    // wrapping: a caller may pass any `i32` (the plan clamps the level, a direct caller may not)
    let seg = |mul: i32, sub: i32| level.wrapping_mul(mul).wrapping_sub(sub);
    let line = if level <= 20 {
        seg(2, 2)
    } else if level <= 40 {
        seg(3, 22)
    } else if level <= 60 {
        seg(4, 62)
    } else if level <= 80 {
        seg(5, 122)
    } else if level <= 100 {
        seg(6, 202)
    } else {
        seg(7, 302)
    };
    let bonus = match u32::try_from(job) {
        Ok(JOB_MERCENARY) => 40,
        Ok(JOB_ACROBAT) => 50,
        Ok(JOB_ASSIST) => 60,
        Ok(JOB_MAGICIAN) => 90,
        Ok(JOB_KNIGHT | JOB_BLADE) => 120,
        Ok(JOB_JESTER | JOB_RANGER) => 150,
        Ok(JOB_RINGMASTER) => 160,
        Ok(JOB_BILLPOSTER | JOB_PSYCHIKEEPER) => 180,
        Ok(JOB_ELEMENTOR) => 390,
        _ => 0,
    };
    line.wrapping_add(bonus)
}

// ---- the fast job change (16 §11.4, F-127) ------------------------------------------------------------------------

/// The jobs a character of `level` and `job` may change to with `UpdateJob` (`features.fast_jobchange`), the
/// strict table of 16 §11.4: a level 15 vagrant to a playable expert (1..=4), a level 60 expert `e` to the pros
/// `(e + 2) * 2` and `(e + 2) * 2 + 1`, a level 120 pro to its master (`pro + 10`), a level 120 master to its
/// hero (`master + 8`). Puppeteer, Gatekeeper and Doppler (5, 14, 15) are never involved, the level-129 hero has
/// no target, everything else is empty. The server validates `UpdateJob` against this list and the browser
/// offers exactly these buttons.
pub fn fast_job_targets(d: &RulesData, level: i32, job: i32) -> SmallVec<[u32; 2]> {
    let mut out = SmallVec::new();
    if level == MAX_JOB_LEVEL && is_base(d, job) && job == JOB_VAGRANT as i32 {
        out.extend(1..=4);
    } else if level == MAX_JOB_LEVEL + MAX_EXP_LEVEL && is_expert(d, job) && (1..=4).contains(&job)
    {
        let first = (job as u32 + 2) * 2;
        out.push(first);
        out.push(first + 1);
    } else if level == MAX_LEVEL && is_pro(d, job) && (6..=13).contains(&job) {
        out.push(job as u32 + 10);
    } else if level == MAX_LEVEL && is_master(d, job) && (16..=23).contains(&job) {
        out.push(job as u32 + 8);
    }
    out
}

/// The level `InitLevel` is called with for the fast job change out of (`level`, `job`): 15 for the vagrant, 60
/// for an expert, 60 for a pro (the master restarts at level 60) and 121 for a master. `None` where
/// [`fast_job_targets`] is empty.
pub fn fast_job_new_level(d: &RulesData, level: i32, job: i32) -> Option<i32> {
    if fast_job_targets(d, level, job).is_empty() {
        None
    } else if is_pro(d, job) {
        Some(MAX_JOB_LEVEL + MAX_EXP_LEVEL)
    } else if is_master(d, job) {
        Some(MAX_LEVEL + 1)
    } else {
        Some(level)
    }
}

/// The client rule that opens the fast job change window (`CMover::SetExperience` and `OnGameJoin`, 16 §18.4):
/// a vagrant of level 15, an expert of level 60, a pro or master of level 120 whose EXP bar shows 99.99 %
/// (`exp_percent_x100 == 9999`), and only where [`fast_job_targets`] has something to offer (the tiers by the
/// fixed `defineJob.h` layout, so the browser needs no job table).
pub fn fast_job_window(level: i32, job: i32, exp_percent_x100: i32) -> bool {
    match (job_type_by_layout(job), level) {
        (JTYPE_BASE, MAX_JOB_LEVEL) => job == JOB_VAGRANT as i32,
        (JTYPE_EXPERT, l) if l == MAX_JOB_LEVEL + MAX_EXP_LEVEL => (1..=4).contains(&job),
        (JTYPE_PRO, MAX_LEVEL) => exp_percent_x100 == 9999 && (6..=13).contains(&job),
        (JTYPE_MASTER, MAX_LEVEL) => exp_percent_x100 == 9999 && (16..=23).contains(&job),
        _ => false,
    }
}
