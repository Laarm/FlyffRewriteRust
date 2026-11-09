//! Jobs, `InitLevel` and the fast job change (Plan/16 §11; WP-MOVER-18; F-126, F-127, PL-083).
//!
//! * [`add_change_job`] is `CMover::AddChangeJob(job)`: the skill fill of the job's tier and `m_nJob`; it sends
//!   nothing.
//! * [`change_job`] is the quest and GM path (`ChangeJob` of the NPC scripts, `/changejob`): `add_change_job`, then
//!   `JobChanged` to the owner and to the observers, the directory, the honor titles, the audit line. Level,
//!   stats and points are untouched.
//! * [`init_level`] is `CMover::InitLevel(job, level, gamma)`: a full rebuild (level, stats 15, GP, SP, skills, the
//!   master and hero skill levels), the message sequence of 16 §8.4 "Job change" and the side effects.
//! * [`on_update_job`] is the fast job change handler (`UpdateJob`, `features.fast_jobchange`): the strict table
//!   [`fast_job_targets`] decides, `init_level` does the work.
//!
//! The arithmetic is `flyff_rules::jobs` (`init_level_plan`, `fast_job_targets`), also used by the browser.

use super::env::{StatsCore, refresh_sim_params};
use super::out::Out;
use super::points::{emit_growth_learning_point, emit_state};
use super::pool::{fill_pools_silent, send_pools_absolute};
use super::progression::emit_experience;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use flyff_proto::pb;
use flyff_rules::consts::MAX_SKILL_JOB;
use flyff_rules::consts::job::JOB_VAGRANT;
use flyff_rules::jobs::{
    InitLevelPlan, SkillSource, SkillTable, fast_job_new_level, fast_job_targets, fill_job_block,
    init_level_plan, is_hero, is_master,
};

/// The base value every stat is reset to by `InitLevel` (`m_nStr = m_nSta = m_nDex = m_nInt = 15`).
const BASE_STAT: i32 = 15;

/// The skill data of the layout functions: `GameData.skills` and the per-job lists.
pub fn skill_source(c: &StatsCore) -> SkillSource<'_> {
    SkillSource {
        skills: &c.data.skills,
        job_skills: &c.data.job_skills,
    }
}

/// `AddChangeJob(job)` on `m` (`MoverParam.cpp:1599-1688`): `m_nJob = job` and the skills of the job's tier from
/// its slot base, at `expert_max` with `features.auto_skill_max` (else 0; masters 1). Level, stats and points do
/// not change and nothing is sent. `false` (nothing changed) for the vagrant, an id above 31 and a non-player.
pub fn add_change_job(m: &mut Mover, c: &StatsCore, job: i32) -> bool {
    let auto = c.cfg.auto_skill_max;
    let Some(p) = m.player_mut() else {
        return false;
    };
    if !fill_job_block(&mut p.progression.job_skills, &skill_source(c), job, auto) {
        return false;
    }
    m.mv.job = job;
    true
}

/// The owner's copy of `JobChanged` (`AddSetChangeJob`): the job and all 45 slots, positional (the legacy
/// `jobLv[32]` is dropped).
pub fn job_changed_self(job: i32, skills: &SkillTable) -> pb::JobChanged {
    pb::JobChanged {
        job,
        skills: skills
            .iter()
            .map(|s| pb::Skill {
                skill_id: s.skill_id,
                level: s.level,
            })
            .collect(),
    }
}

/// The observers' copy of `JobChanged` (`AddNearSetChangeJob`, `User.cpp:5148`): the legacy subset - 21 slots from
/// slot 20 for masters and heroes, otherwise 20 slots from slot 3. The list is display-only (16 §11.5).
pub fn job_changed_observer(c: &StatsCore, job: i32, skills: &SkillTable) -> pb::JobChanged {
    let (from, count) = if is_master(&c.rules, job) || is_hero(&c.rules, job) {
        (20, 21)
    } else {
        (3, 20)
    };
    pb::JobChanged {
        job,
        skills: skills
            .iter()
            .skip(from)
            .take(count)
            .map(|s| pb::Skill {
                skill_id: s.skill_id,
                level: s.level,
            })
            .collect(),
    }
}

/// Queues the two `JobChanged` copies: the owner's, then the observers' (`NearExclSelf`).
pub fn emit_job_changed(m: &Mover, c: &StatsCore, out: &mut Out) {
    let Some(p) = m.player() else { return };
    out.to_self(job_changed_self(m.mv.job, &p.progression.job_skills));
    out.near_excl(job_changed_observer(c, m.mv.job, &p.progression.job_skills));
}

/// What [`apply_init_level`] changed, for the callers' side effects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitLevelDone {
    /// The job before.
    pub old_job: i32,
    /// The level before.
    pub old_level: i32,
    /// The plan that was applied.
    pub plan: InitLevelPlan,
}

/// The mover part of `InitLevel(job, level, gamma)` (`Mover.cpp:1598-1873`, 16 §11.3): the plan of the rules is
/// copied into the player - job, level, stats 15, `stat_points`, `skill_points` = `skill_level_total`, the skill
/// table, `death_level` (the death EXP stays) and, with `gamma`, `exp = 0` - the pools are refilled silently and
/// the sim parameters follow. Then the messages of 16 §8.4 "Job change" are queued in order: `JobChanged` (owner,
/// observers), `SetLevel` (observers), `SetGrowthLearningPoint`, `SetExperience`, `TaskBar` (the skill shortcuts
/// are gone), `SetState`; the forced pools of PL-083 follow with [`finish_init_level`] after the honor messages
/// of doc 21.
///
/// Returns `None` for a non-player.
pub fn apply_init_level(
    m: &mut Mover,
    c: &StatsCore,
    job: i32,
    level: i32,
    gamma: bool,
    out: &mut Out,
) -> Option<InitLevelDone> {
    let old_job = m.mv.job;
    let old_level = m.mv.level;
    m.player()?;
    let plan = init_level_plan(&c.rules, &skill_source(c), job, level, c.cfg.auto_skill_max);
    m.mv.job = plan.job;
    m.mv.level = plan.level;
    m.mv.base.str = BASE_STAT;
    m.mv.base.sta = BASE_STAT;
    m.mv.base.dex = BASE_STAT;
    m.mv.base.int = BASE_STAT;
    if let Some(p) = m.player_mut() {
        let pr = &mut p.progression;
        pr.stat_points = plan.stat_points;
        pr.skill_points = plan.skill_points;
        pr.skill_level_total = plan.skill_points;
        pr.job_skills = plan.skills;
        pr.death_level = plan.death_level;
        pr.max_level = pr
            .max_level
            .max(u16::try_from(plan.level).unwrap_or(u16::MAX));
        if gamma {
            pr.exp = 0;
        }
    }
    // the pools at the new maxima (silently: the forced absolute messages end the sequence)
    refresh_sim_params(m, c);
    fill_pools_silent(m, c);

    emit_job_changed(m, c, out);
    out.near_excl(pb::SetLevel {
        level: u32::try_from(plan.level).unwrap_or(0),
    });
    emit_growth_learning_point(m, out);
    emit_experience(m, out);
    if let Some(p) = m.player_mut() {
        crate::taskbar::reset_skill_shortcuts(&mut p.taskbar);
    }
    if let Some(tb) = crate::taskbar::player_taskbar_msg(m) {
        out.to_self(tb);
    }
    emit_state(m, out);
    Some(InitLevelDone {
        old_job,
        old_level,
        plan,
    })
}

/// The last step of the job-change sequence: the three forced `SetPointParam` (PL-083, `pl_083_absolute_hp`).
pub fn finish_init_level(m: &mut Mover, c: &StatsCore, out: &mut Out) {
    send_pools_absolute(m, c, out);
}

/// `InitLevel(job, level, gamma)` for a player with the whole sequence of 16 §8.4 "Job change": the mover
/// rebuild and its messages ([`apply_init_level`]), the honor titles (doc 21: `CheckHonorStat`, the list and the
/// title near), the forced pools, then the directory entry (`UpdatePlayerData`), the audit line `char.job_change`
/// and an urgent save. `level` is clamped to the rows of the EXP table. Returns `false` when `who` is not a live
/// player.
pub fn init_level(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    job: i32,
    level: i32,
    gamma: bool,
) -> bool {
    let core = svc.stats.handle();
    let mut out = Out::new();
    let done = {
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return false;
        };
        apply_init_level(m, &core, job, level, gamma, &mut out)
    };
    let Some(done) = done else { return false };
    out.flush(ctx, who.0);
    // doc 21: the honor titles follow the new stats and job (their messages come before the pools)
    crate::social::hooks::honor_recheck(svc, ctx, who, true);
    let mut tail = Out::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        finish_init_level(m, &core, &mut tail);
    }
    tail.flush(ctx, who.0);
    after_job_change(svc, ctx, who, &done);
    true
}

/// The side effects that follow a job change: the player directory (level and job), the audit line and the urgent
/// save.
fn after_job_change(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    done: &InitLevelDone,
) {
    let (level, job) = (done.plan.level, done.plan.job);
    crate::social::hooks::on_level_changed(
        svc,
        ctx,
        who,
        u32::try_from(done.old_level).unwrap_or(0),
        u32::try_from(level).unwrap_or(0),
    );
    crate::social::hooks::on_job_changed(svc, ctx, who, u8::try_from(job).unwrap_or(u8::MAX));
    audit_job_change(ctx, who, done.old_job, job, level);
    svc.saves.mark_urgent(who.0);
}

fn audit_job_change(ctx: &mut GameCtx<'_>, who: PlayerRef, from: i32, to: i32, level: i32) {
    crate::audit::emit_for(
        ctx,
        who,
        flyff_db::audit::ev::CHAR_JOB_CHANGE,
        serde_json::json!({ "from": from, "job": to, "level": level }),
    );
}

/// `jobs::change_job(p, job)` (16 §11.4 last paragraph): [`add_change_job`] plus the `JobChanged` messages, the
/// directory, the honor titles, the audit line and an urgent save; used by the NPC script helper `ChangeJob` and
/// the GM command `/changejob`. No level, stat or point changes. Returns `false` (nothing happened) when the job is
/// rejected by `AddChangeJob` or `who` is not a live player.
pub fn change_job(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, job: i32) -> bool {
    let core = svc.stats.handle();
    let mut out = Out::new();
    let (old_job, level) = {
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return false;
        };
        let old = m.mv.job;
        if !add_change_job(m, &core, job) {
            return false;
        }
        emit_job_changed(m, &core, &mut out);
        (old, m.mv.level)
    };
    out.flush(ctx, who.0);
    crate::social::hooks::honor_recheck(svc, ctx, who, true);
    crate::social::hooks::on_job_changed(svc, ctx, who, u8::try_from(job).unwrap_or(u8::MAX));
    audit_job_change(ctx, who, old_job, job, level);
    svc.saves.mark_urgent(who.0);
    true
}

/// `UpdateJob{job}` (legacy `UPDATE_JOB 0xAA000001`, `OnUpdateJob`, 16 §11.4, `features.fast_jobchange`): accepted
/// only for the four transitions of [`fast_job_targets`] - a level 15 vagrant to an expert 1..=4, a level 60
/// expert to one of its two pros, a level 120 pro to its master, a level 120 master to its hero - and then
/// `InitLevel(job, 15 | 60 | 60 | 121, gamma)`. Everything else (the legacy hack range 0/5/6, a wrong pro pair,
/// the level 129 hero, a dead player) is ignored with a warning, no kick. With `features.fast_jobchange = false`
/// the message is ignored.
pub fn on_update_job(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::UpdateJob) {
    let core = svc.stats.handle();
    if !core.fast_jobchange {
        return;
    }
    let Some(mv) = ctx.worlds.player(who.0) else {
        return;
    };
    let (level, job, alive) = (mv.mv.level, mv.mv.job, mv.mv.hp > 0);
    let allowed = alive
        && u32::try_from(m.job)
            .is_ok_and(|target| fast_job_targets(&core.rules, level, job).contains(&target));
    let new_level = fast_job_new_level(&core.rules, level, job);
    let (true, Some(new_level)) = (allowed, new_level) else {
        tracing::warn!(
            level,
            job,
            requested = m.job,
            alive,
            "invalid fast job change"
        );
        return;
    };
    init_level(svc, ctx, who, m.job, new_level, true);
}

/// `JOB_VAGRANT` as `i32` (the job of a character that never changed).
pub const VAGRANT: i32 = JOB_VAGRANT as i32;

/// The number of slots in the skill table (re-exported for the tests).
pub const SKILL_SLOTS: usize = MAX_SKILL_JOB;
