//! Skills and party skills (Plan/14 §6.4, §5.4 F4; ref/17 §7.2.2; src:_Common/Project.cpp:378-415).
//!
//! `propSkill.txt` (190 rows, ids `SI_*`) and `propTroupeSkill.txt` (8 rows, ids `ST_*`) use the item
//! row loader. This module adds the per-job skill lists (`LoadSkill`): each job's skills ordered by the
//! required level.

use crate::props::item::{ItemProp, SkillProp};
use crate::types::{IdTable, JobId, NULL_ID, SkillId};
use flyff_common::defs::job;
use serde::{Deserialize, Serialize};

/// `MAX_JOB`.
pub const MAX_JOB: usize = job::MAX_JOB as usize;
/// The capacity of one job's skill list in the original (`m_aJobSkill[MAX_JOB][40]`).
pub const MAX_JOB_SKILLS: usize = 40;

/// The skills of every job, `by_job[JOB_*]` (F4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobSkills {
    /// `[MAX_JOB = 32]` lists of at most [`MAX_JOB_SKILLS`] skill ids, sorted by `req_dis_lv`
    /// ascending, ties by ascending skill id.
    pub by_job: Vec<Vec<SkillId>>,
}

impl Default for JobSkills {
    fn default() -> Self {
        Self {
            by_job: vec![Vec::new(); MAX_JOB],
        }
    }
}

impl JobSkills {
    /// The skills of `job` in list order (empty for an unknown job).
    pub fn of_job(&self, job: JobId) -> &[SkillId] {
        usize::try_from(job)
            .ok()
            .and_then(|i| self.by_job.get(i))
            .map_or(&[], Vec::as_slice)
    }
}

/// Findings of [`build_job_skills`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobSkillsReport {
    /// Skills whose owning job (`item_kind2`) is `>= MAX_JOB`: `(skill id, job)`.
    pub bad_job: Vec<(SkillId, u32)>,
    /// Skills dropped because the job already had [`MAX_JOB_SKILLS`]: `(skill id, job)`.
    pub overflow: Vec<(SkillId, u32)>,
}

/// `CProject::LoadSkill` after the file load: for skill ids >= 1 ascending with
/// `item_kind1 != JTYPE_COMMON`, push the id into `by_job[item_kind2]` (at most 40 per job) and sort every
/// list by `req_dis_lv` ascending with a **stable** sort whose tie-break is the skill id.
///
/// **FIX (F4, ledger PL-016):** the original `qsort` has an unspecified tie order (MSVC); the rewrite
/// orders ties by ascending skill id so two loads always give the same lists.
pub fn build_job_skills(skills: &IdTable<SkillProp>) -> (JobSkills, JobSkillsReport) {
    let mut out = JobSkills::default();
    let mut report = JobSkillsReport::default();
    for (id, s) in skills.iter() {
        if id == 0 || s.item_kind1 == job::JTYPE_COMMON {
            continue;
        }
        let job = s.item_kind2;
        match out.by_job.get_mut(job as usize) {
            None => report.bad_job.push((id, job)),
            Some(list) if list.len() >= MAX_JOB_SKILLS => report.overflow.push((id, job)),
            Some(list) => list.push(id),
        }
    }
    for list in &mut out.by_job {
        list.sort_by_key(|id| {
            (
                skills.get(*id).map_or(NULL_ID, |s: &ItemProp| s.req_dis_lv),
                *id,
            )
        });
    }
    (out, report)
}
