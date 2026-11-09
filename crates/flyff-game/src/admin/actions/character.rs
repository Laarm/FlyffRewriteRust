//! Character edits of the console (Plan/25 §4.6, §11.5, §11.10): level, job, stats, skills, quests, rename.
//! **WP-ADMIN-12** implements the edits of an online character with the services the GM commands use (`/level`, `/changejob`,
//! `/stat`, `/InitSkillExp`, 23 rows 52-56 and 62); `Rename` is **WP-ADMIN-16** (21 §3.3) and `Quest` is Phase C
//! (**WP-ADMIN-31**): both answer `not_implemented`.
//!
//! An edit needs the character **online and alive** (the pools are refilled by a level or job change, which must not
//! half-revive a corpse) and passes rank protection. The answer is a one-entry report whose info says what changed:
//! `{field, before, after}`.

use super::common::{check_rank, fail, online_player, report};
use super::{TargetId, TargetResult, not_implemented};
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::combat::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::cmd::progress::{LevelRule, level_rule};
use crate::input::{AdminActor, AdminOutput};
use crate::stats::Out;
use crate::stats::points::{emit_state, restat_full, stats_changed};
use crate::stats::pool::send_pools_absolute;
use crate::stats::progression::{add_skill_points, emit_experience};
use flyff_common::CharacterId;
use flyff_rules::consts::job::{JOB_VAGRANT, MAX_JOB, MAX_LEVEL};
use serde_json::{Value, json};

/// The largest base stat the console sets (the character-sheet field is four digits).
pub const MAX_BASE_STAT: u16 = 9999;

/// What a quest edit does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestOp {
    /// Start the quest.
    Start,
    /// Complete it.
    Complete,
    /// Remove it from the quest book.
    Remove,
}

/// One edit of an online character (25 §11.10 `CharacterEdit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CharacterEdit {
    /// Set the level (and optionally the job). The rule is `/level`'s: up to level 15 the character is a vagrant,
    /// 16..=60 needs an expert job, 61..=120 a professional one. Without `job` the character keeps its job.
    Level {
        /// The new level.
        level: u16,
        /// A job to switch to as well.
        job: Option<u8>,
    },
    /// Change the job (`/changejob`: the job-change rules of 16 §11.4, no level, stat or point changes).
    Job(u8),
    /// Set the base stats (`/stat`).
    Stats {
        /// Strength.
        str: u16,
        /// Stamina.
        sta: u16,
        /// Dexterity.
        dex: u16,
        /// Intelligence.
        int: u16,
    },
    /// Reset the stats and refund the points (`/stat restate`).
    Restat,
    /// Add (or remove, when negative) skill points. A grant stops at the cap of the job line (16 §12.2).
    SkillPoints(i32),
    /// Forget all skills and refund the points (`/InitSkillExp`).
    ResetSkills,
    /// Rename (WP-ADMIN-16).
    Rename(String),
    /// A quest edit (Phase C).
    Quest {
        /// What to do.
        op: QuestOp,
        /// The quest.
        quest: u32,
    },
}

impl CharacterEdit {
    /// The name of the edited field in the audit (`admin.character_edit.field`).
    pub const fn field(&self) -> &'static str {
        match self {
            CharacterEdit::Level { .. } => "level",
            CharacterEdit::Job(_) => "job",
            CharacterEdit::Stats { .. } => "stats",
            CharacterEdit::Restat => "restat",
            CharacterEdit::SkillPoints(_) => "skill_points",
            CharacterEdit::ResetSkills => "skills",
            CharacterEdit::Rename(_) => "name",
            CharacterEdit::Quest { .. } => "quest",
        }
    }
}

/// Edit an online character (`EditCharacter`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditCharacter {
    /// The character.
    pub character: CharacterId,
    /// The edit.
    pub edit: CharacterEdit,
}

/// A snapshot of the fields an edit may change, for `before` and `after` of the audit.
fn snapshot(ctx: &GameCtx<'_>, who: PlayerRef) -> Value {
    let Some(m) = ctx.worlds.player(who.0) else {
        return Value::Null;
    };
    let (gp, sp) = m.player().map_or((0, 0), |p| {
        (p.progression.stat_points, p.progression.skill_points)
    });
    json!({
        "level": m.mv.level,
        "job": m.mv.job,
        "str": m.mv.base.str,
        "sta": m.mv.base.sta,
        "dex": m.mv.base.dex,
        "int": m.mv.base.int,
        "stat_points": gp,
        "skill_points": sp,
    })
}

/// The `before`/`after` pair of an edit, restricted to the keys the edit can touch.
fn changed(before: &Value, after: &Value, keys: &[&str]) -> (Value, Value) {
    let pick = |v: &Value| {
        let mut o = serde_json::Map::new();
        for k in keys {
            if let Some(x) = v.get(*k) {
                o.insert((*k).to_owned(), x.clone());
            }
        }
        Value::Object(o)
    };
    (pick(before), pick(after))
}

/// `EditCharacter`: see the module documentation.
pub(super) fn edit_character(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: EditCharacter,
) -> AdminOutput {
    match &cmd.edit {
        CharacterEdit::Rename(_) => return not_implemented("rename a character", "WP-ADMIN-16"),
        CharacterEdit::Quest { .. } => return not_implemented("edit a quest", "WP-ADMIN-31"),
        _ => {}
    }
    let target = TargetId::Character(cmd.character);
    let r = (|| -> Result<Value, AdminFailure> {
        let who = online_player(svc, ctx, cmd.character)?;
        check_rank(&actor, ctx, who.0)?;
        if ctx
            .worlds
            .player(who.0)
            .is_none_or(|m| is_die(m) || m.mv.hp <= 0)
        {
            return Err(fail(
                AdminErrorCode::StaleTarget,
                format!(
                    "character {} is dead: resurrect it before editing",
                    cmd.character.0
                ),
            ));
        }
        let before = snapshot(ctx, who);
        let keys = apply(svc, ctx, who, &cmd.edit)?;
        let after = snapshot(ctx, who);
        let (b, a) = changed(&before, &after, keys);
        Ok(json!({"field": cmd.edit.field(), "before": b, "after": a}))
    })();
    report(vec![match r {
        Ok(info) => TargetResult::ok(target).with_info(info),
        Err(f) => TargetResult::failed(target, f),
    }])
}

/// Applies one edit; returns the keys of [`snapshot`] it can have changed.
fn apply(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    edit: &CharacterEdit,
) -> Result<&'static [&'static str], AdminFailure> {
    match edit {
        CharacterEdit::Level { level, job } => {
            apply_level(svc, ctx, who, *level, *job)?;
            Ok(&[
                "level",
                "job",
                "str",
                "sta",
                "dex",
                "int",
                "stat_points",
                "skill_points",
            ])
        }
        CharacterEdit::Job(job) => {
            if u32::from(*job) >= MAX_JOB {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    format!("job {job} does not exist"),
                ));
            }
            if !crate::stats::jobs::change_job(svc, ctx, who, i32::from(*job)) {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    format!("the job change to {job} is not allowed from the current job"),
                ));
            }
            Ok(&["job"])
        }
        CharacterEdit::Stats { str, sta, dex, int } => {
            apply_stats(svc, ctx, who, [*str, *sta, *dex, *int])?;
            Ok(&["str", "sta", "dex", "int"])
        }
        CharacterEdit::Restat => {
            let core = svc.stats.handle();
            let mut out = Out::new();
            let done = ctx
                .worlds
                .player_mut(who.0)
                .is_some_and(|m| restat_full(m, &core, &mut out).is_some());
            // the scroll refuses (with a text) when every stat is at the base already: nothing changes, nothing is
            // sent, and the console just reports the same before and after
            if done {
                out.flush(ctx, who.0);
            }
            Ok(&["str", "sta", "dex", "int", "stat_points"])
        }
        CharacterEdit::SkillPoints(delta) => {
            let core = svc.stats.handle();
            let mut out = Out::new();
            if let Some(m) = ctx.worlds.player_mut(who.0) {
                if *delta >= 0 {
                    add_skill_points(m, &core, *delta);
                } else if let Some(p) = m.player_mut() {
                    p.progression.skill_points =
                        p.progression.skill_points.saturating_add(*delta).max(0);
                }
                emit_experience(m, &mut out);
            }
            out.flush(ctx, who.0);
            Ok(&["skill_points"])
        }
        CharacterEdit::ResetSkills => {
            let core = svc.stats.handle();
            let mut out = Out::new();
            let done = ctx
                .worlds
                .player_mut(who.0)
                .is_some_and(|m| crate::stats::skills::init_skill_exp(m, &core, &mut out));
            if !done {
                return Err(fail(
                    AdminErrorCode::Internal,
                    "a learned skill has no skill prop: the reset was refused",
                ));
            }
            out.flush(ctx, who.0);
            Ok(&["skill_points"])
        }
        CharacterEdit::Rename(_) | CharacterEdit::Quest { .. } => {
            Err(fail(AdminErrorCode::NotImplemented, "not implemented yet"))
        }
    }
}

/// `Level{level, job}` with the `/level` rule ([`level_rule`]) and the `/level` service (`jobs::init_level`).
fn apply_level(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    level: u16,
    job: Option<u8>,
) -> Result<(), AdminFailure> {
    let level = i32::from(level);
    if !(1..=MAX_LEVEL as i32).contains(&level) {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("the level must be 1..={MAX_LEVEL}"),
        ));
    }
    let current = ctx.worlds.player(who.0).map_or(JOB_VAGRANT, |m| {
        u32::try_from(m.mv.job).unwrap_or(JOB_VAGRANT)
    });
    let asked = job.map_or(current, u32::from);
    if asked >= MAX_JOB {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("job {asked} does not exist"),
        ));
    }
    match level_rule(asked, level) {
        LevelRule::Apply { job } => {
            if crate::stats::jobs::init_level(svc, ctx, who, job as i32, level, true) {
                Ok(())
            } else {
                Err(fail(
                    AdminErrorCode::StaleTarget,
                    "the character left the world",
                ))
            }
        }
        LevelRule::NotExpert => Err(fail(
            AdminErrorCode::Invalid,
            format!("level {level} needs an expert job: send the job as well"),
        )),
        LevelRule::NotProfessional => Err(fail(
            AdminErrorCode::Invalid,
            format!("level {level} needs a professional job: send the job as well"),
        )),
        LevelRule::Ignored => Err(fail(
            AdminErrorCode::Invalid,
            format!("the level must be 1..={MAX_LEVEL}"),
        )),
    }
}

/// `Stats{..}`: the four base stats become the given values (`/stat str|sta|dex|int`), the derived values follow (the
/// pools are clamped to the new maxima and sent, the sim parameters refreshed), `SetState` goes to the owner and the
/// honor titles are re-checked.
fn apply_stats(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    v: [u16; 4],
) -> Result<(), AdminFailure> {
    if v.iter().any(|s| *s == 0 || *s > MAX_BASE_STAT) {
        return Err(fail(
            AdminErrorCode::Invalid,
            format!("every stat must be 1..={MAX_BASE_STAT}"),
        ));
    }
    let core = svc.stats.handle();
    let mut out = Out::new();
    {
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return Err(fail(
                AdminErrorCode::StaleTarget,
                "the character left the world",
            ));
        };
        m.mv.base.str = i32::from(v[0]);
        m.mv.base.sta = i32::from(v[1]);
        m.mv.base.dex = i32::from(v[2]);
        m.mv.base.int = i32::from(v[3]);
        stats_changed(m, &core);
        send_pools_absolute(m, &core, &mut out);
        emit_state(m, &mut out);
    }
    out.flush(ctx, who.0);
    crate::social::hooks::honor_recheck(svc, ctx, who, true);
    Ok(())
}
