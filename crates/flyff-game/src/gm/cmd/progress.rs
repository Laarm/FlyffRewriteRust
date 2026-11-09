//! The skill, job, PvP, buff and disguise commands (Plan/23 §4.5, §4.6; WP-GM-18): `/level` (row 53), `/PvpParam`
//! (44), `/PKParam` (45), `/InitSkillExp` (54), `/skilllevel` (55), `/skilllevelAll` (56), `/changejob` (62),
//! `/disguise` (79), `/noDisguise` (80), `/RemoveBuff` (112) and `/AExp` (154). Item-triggered runs of the item
//! privileged ones (`/dis`, `/nodis`) are WP-GM-19's.
//!
//! The commands act on the issuer's character ("self"): a player, an item user, or the `as <name>` character of
//! the console. Names are matched exact first, then case-insensitively, then as a define name (`JOB_KNIGHT`,
//! `MI_MALE`; PL-099, 23 §13 Q2).

use crate::ctx::PlayerRef;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::stats::Out;
use flyff_common::defs::text::TID_GAME_CHOICEJOB;
use flyff_data::GameData;
use flyff_proto::pb;
use flyff_rules::consts::job::{
    JOB_VAGRANT, MAX_EXP_LEVEL, MAX_EXPERT, MAX_JOB, MAX_JOB_LEVEL, MAX_JOBBASE, MAX_LEVEL,
    MAX_PROFESSIONAL,
};

/// The issuer's character.
fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// A name against a stored name: exact (`strcmp`), or ASCII case-insensitive when `fold`.
fn name_matches(stored: &str, token: &str, fold: bool) -> bool {
    crate::gm::lookup::name_matches(stored, token, fold)
}

/// The job a command word names; see [`crate::gm::lookup::job`].
pub fn find_job(gd: &GameData, token: &str) -> Option<usize> {
    crate::gm::lookup::job(gd, token)
}

/// The display name of job `i` (`""` when the table has no row).
fn job_name(gd: &GameData, i: usize) -> &str {
    gd.jobs
        .info
        .get(i)
        .and_then(Option::as_ref)
        .map_or("", |j| j.name.as_str())
}

/// The names of the jobs `from..to` as the list lines of `/level` print them: names shorter than 15 bytes, joined
/// with `, ` (the separator is only added after a printed name that is not the last job, like the source).
fn job_list(gd: &GameData, from: u32, to: u32) -> String {
    let mut s = String::new();
    for i in from..to {
        let name = job_name(gd, i as usize);
        if name.len() < 15 {
            s.push_str(name);
            if i + 1 != to {
                s.push_str(", ");
            }
        }
    }
    s
}

/// What `/level` does for a job and a level (the part of `TextCmd_Level` after the legend branch): shared with the
/// admin console's level edit (25 §4.6), which must apply exactly the same rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelRule {
    /// `InitLevel(job, level)` runs with this job (up to level 15 the job is the vagrant whatever was asked).
    Apply {
        /// The job to initialise.
        job: u32,
    },
    /// Levels 16..=60 need an expert job (`Not Expert Job` and the list).
    NotExpert,
    /// Levels 61..=120 need a professional job (`Not Professional Job` and the list).
    NotProfessional,
    /// A level above the maximum: nothing happens.
    Ignored,
}

/// The `/level` rule: up to `MAX_JOB_LEVEL` the character becomes a vagrant, `16..=60` needs an expert job (a
/// professional is accepted at exactly 60: the repack's fast job change creates it there), `61..=120` a professional
/// one, above that nothing happens.
pub fn level_rule(job: u32, level: i32) -> LevelRule {
    let expert_cap = (MAX_JOB_LEVEL + MAX_EXP_LEVEL) as i32;
    if level <= MAX_JOB_LEVEL as i32 {
        LevelRule::Apply { job: JOB_VAGRANT }
    } else if level <= expert_cap {
        let expert = (MAX_JOBBASE..MAX_EXPERT).contains(&job);
        let pro_at_60 = level == expert_cap && (MAX_EXPERT..MAX_PROFESSIONAL).contains(&job);
        if expert || pro_at_60 {
            LevelRule::Apply { job }
        } else {
            LevelRule::NotExpert
        }
    } else if level <= MAX_LEVEL as i32 {
        if (MAX_EXPERT..MAX_PROFESSIONAL).contains(&job) {
            LevelRule::Apply { job }
        } else {
            LevelRule::NotProfessional
        }
    } else {
        LevelRule::Ignored
    }
}

/// Row 53 `/level (lv) <jobName> <level> [legend 1/2]` (`TextCmd_Level`, `FuncTextCmd.cpp:733`, 16 §11.3): the job is
/// the first one with that name (the vagrant when unknown), `level` 0 becomes 1. A level above 15 for the vagrant
/// answers `TID_GAME_CHOICEJOB`. `legend` 1 or 2 picks the first or second later job with the same name (the
/// master and the hero) and runs `init_level` without a range check. Otherwise `level <= 15` is a vagrant,
/// `16..=60` needs an expert job (`Not Expert Job` and the list otherwise), `61..=120` a professional one (also
/// `60` for the repack's fast job change, which creates a pro at level 60), and above 120 nothing happens.
pub fn level(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let token = args.token().to_owned();
    let mut level = args.number();
    if level == 0 {
        level = 1;
    }
    let data = std::sync::Arc::clone(cx.ctx.data);
    let found = find_job(&data, &token);
    let job = found.unwrap_or(JOB_VAGRANT as usize) as u32;
    if level > MAX_JOB_LEVEL as i32 && job == JOB_VAGRANT {
        cx.out.text(data.texts.get(TID_GAME_CHOICEJOB));
        return Ok(CmdDone::default());
    }
    let legend = args.number();
    if legend > 0 && legend < 3 {
        // the 1st / 2nd later job with the same name: the master and the hero
        let name = job_name(&data, job as usize).to_owned();
        let mut j = job;
        let mut want = legend;
        for i in (job + 1)..MAX_JOB {
            if !name.is_empty() && name_matches(job_name(&data, i as usize), &name, false) {
                j = i;
                if want == 1 {
                    break;
                }
                want -= 1;
            }
        }
        crate::stats::jobs::init_level(cx.svc, cx.ctx, who, j as i32, level, true);
        return Ok(CmdDone::default());
    }
    match level_rule(job, level) {
        LevelRule::Apply { job } => {
            crate::stats::jobs::init_level(cx.svc, cx.ctx, who, job as i32, level, true);
        }
        LevelRule::NotExpert => {
            cx.out.text("Not Expert Job");
            cx.out.text(format!(
                "Expert Job : {}",
                job_list(&data, MAX_JOBBASE, MAX_EXPERT)
            ));
            cx.out.text(format!(
                "Expert Level : {} ~ {}",
                MAX_JOB_LEVEL + 1,
                MAX_JOB_LEVEL + MAX_EXP_LEVEL
            ));
        }
        LevelRule::NotProfessional => {
            cx.out.text("Not Professional Job");
            cx.out.text(format!(
                "Professional Job : {}",
                job_list(&data, MAX_EXPERT, MAX_PROFESSIONAL)
            ));
            cx.out.text(format!(
                "Professional Level : {} ~~~ ",
                MAX_JOB_LEVEL + MAX_EXP_LEVEL + 1
            ));
        }
        LevelRule::Ignored => {}
    }
    Ok(CmdDone::default())
}

/// Row 44 `/PvpParam (p_Param) <fame> [slaughter]` (`TextCmd_PvpParam`): the fame becomes `fame` and goes to the
/// near set as `SetFame`; the slaughter argument is read and ignored (version 15).
pub fn pvp_param(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let fame = args.number();
    let _slaughter = args.number();
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0) {
        crate::stats::fame_fuel::set_fame(m, fame, &mut out);
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 45 `/PKParam (pkparam) <pkValue> <propensity>` (`TextCmd_PKParam`): each value is applied only when it is
/// at least 0. The PK value is stored and sent to its owner (`PkRelation{PKVALUE}`) and the honor titles are
/// re-checked; the propensity is stored and sent to the near set (`PkRelation{PROPENSITY}`).
pub fn pk_param(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let pk_value = args.number();
    let propensity = args.number();
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0)
        && let Some(p) = m.player_mut()
    {
        if pk_value >= 0 {
            p.pk.value = pk_value;
            out.to_self(pb::PkRelation {
                sub: Some(pb::pk_relation::Sub::PkValue(pb::pk_relation::PkPkValue {
                    pk_value,
                })),
            });
        }
        if let Ok(prop) = u32::try_from(propensity) {
            p.pk.propensity = prop;
            out.near(pb::PkRelation {
                sub: Some(pb::pk_relation::Sub::Propensity(
                    pb::pk_relation::PkPropensity { propensity: prop },
                )),
            });
        }
    }
    out.flush(cx.ctx, who.0);
    if pk_value >= 0 {
        crate::social::hooks::honor_recheck(cx.svc, cx.ctx, who, true);
    }
    Ok(CmdDone::default())
}

/// Row 54 `/InitSkillExp (InitSE)`: the skill reset - every skill but the master and hero skills back to level 0,
/// the points refunded (clamped to the job line's cap), the skill shortcuts gone from the taskbar,
/// `InitSkillPoint{points}` to the owner.
pub fn init_skill_exp(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let core = cx.svc.stats.handle();
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0) {
        crate::stats::skills::init_skill_exp(m, &core, &mut out);
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 55 `/skilllevel (slv) <skillId> <level>` (server form): sets the level of one of the issuer's own skills and
/// sends `CmdSetSkillLevel{skill, level}` to the owner. FIX (PL-115): the skill must be in the issuer's job-skill
/// table (otherwise nothing happens, silently like the legacy `FALSE`) and the level is clamped to
/// `1..=expert_max`.
pub fn skill_level(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let skill = args.number() as u32;
    let wanted = args.number() as u32;
    let data = std::sync::Arc::clone(cx.ctx.data);
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0)
        && let Some(p) = m.player_mut()
        && let Some(slot) = p
            .progression
            .job_skills
            .iter_mut()
            .find(|s| !s.is_empty() && s.skill_id == skill)
    {
        let max = data.skills.get(skill).map_or(0, |s| s.expert_max);
        slot.level = wanted.max(1).min(max);
        out.to_self(pb::CmdSetSkillLevel {
            skill_id: slot.skill_id,
            level: slot.level,
        });
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 56 `/skilllevelAll (slvAll)`: every job skill of the issuer goes to its `expert_max` (a skill without a prop
/// is skipped); one `CmdSetSkillLevel` per skill.
pub fn skill_level_all(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let data = std::sync::Arc::clone(cx.ctx.data);
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0)
        && let Some(p) = m.player_mut()
    {
        for slot in p
            .progression
            .job_skills
            .iter_mut()
            .filter(|s| !s.is_empty())
        {
            let Some(prop) = data.skills.get(slot.skill_id) else {
                continue;
            };
            slot.level = prop.expert_max;
            out.to_self(pb::CmdSetSkillLevel {
                skill_id: slot.skill_id,
                level: slot.level,
            });
        }
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 62 `/changejob (cjob) <jobName>` (`TextCmd_ChangeJob`): `AddChangeJob` of the named job (the first of that
/// name; the define names pick masters and heroes) with the `JobChanged` messages, the directory update and the
/// audit line; no level, stat or point changes. An unknown name (the vagrant) prints `Error Job Name or Number`,
/// a job `AddChangeJob` rejects `Error 1ch -> 2ch`.
pub fn change_job(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let token = args.token().to_owned();
    let job = find_job(cx.ctx.data, &token).unwrap_or(JOB_VAGRANT as usize);
    if job == JOB_VAGRANT as usize {
        cx.out.text("Error Job Name or Number");
        return Ok(CmdDone::default());
    }
    if !crate::stats::jobs::change_job(cx.svc, cx.ctx, who, job as i32) {
        cx.out.text("Error 1ch -> 2ch");
    }
    Ok(CmdDone::default())
}

/// The mover a command word names (`/disguise`); see [`crate::gm::lookup::mover`].
fn find_mover(gd: &GameData, token: &str, is_number: bool) -> Option<u32> {
    crate::gm::lookup::mover(gd, token, is_number)
}

/// Row 79 `/disguise (dis) <moverId or name>` (`TextCmd_Disguise`, item-privileged): the issuer looks like that
/// mover (`Disguise{mover_index}` to the near set). An unknown mover does nothing.
pub fn disguise(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let token = args.token().to_owned();
    let is_number = args.token_is_number();
    let Some(id) = find_mover(cx.ctx.data, &token, is_number) else {
        return Ok(CmdDone::default());
    };
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0) {
        crate::stats::fashion::set_disguise(m, id, &mut out);
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 80 `/noDisguise (nodis)` (`TextCmd_NoDisguise`, item-privileged): the disguise ends (`NoDisguise` to the
/// near set, also when there was none, like the legacy).
pub fn no_disguise(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0) {
        crate::stats::fashion::clear_disguise(m, &mut out);
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 112 `/RemoveBuff (rb)` (`TextCmd_RemoveAllBuff`): `RemoveAllBuff` (every buff but the housing ones) and
/// `ClearAllSMMode` (every premium timer too, KEEP).
pub fn remove_buff(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    crate::buff::remove_all(cx.svc, cx.ctx, who.0);
    let core = cx.svc.stats.handle();
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0) {
        crate::stats::sm::clear_all(m, &core, &mut out);
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}

/// Row 154 `/AExp (aexp) <n>` (`TextCmd_AngelExp`): with an angel buff the angel EXP grows by `n`, capped at the
/// buff's share (`nAdjParamVal1` percent, 100 when unset or out of range) of the EXP of the angel level
/// (`exp1 / 100 * percent`); a full angel is announced `complete`. `Angel{state 1, exp, level, complete}` to the
/// owner. Nothing happens without the buff or when the angel EXP is already at its cap.
pub fn angel_exp(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let add = i64::from(args.number());
    let core = cx.svc.stats.handle();
    let mut out = Out::new();
    if let Some(m) = cx.ctx.worlds.player_mut(who.0) {
        crate::stats::progression::add_angel_exp(m, &core, add, &mut out);
    }
    out.flush(cx.ctx, who.0);
    Ok(CmdDone::default())
}
