//! `JobProp`, `JobInfo` and `LoadPropJob` (Plan/14 §6.6, ref/17 §7.6; src:_Common/Project.cpp:4954-4995).
//!
//! `propJob.inc` is ASCII, 32 rows (`JOB_VAGRANT` .. `JOB_ELEMENTOR_HERO`) of 19 tokens: the job id and 18
//! floats. The BOW column (`fMeleeBOW`, custom `__BOWDMG_PROPJOB`) sits between YOYO and BLOCK in the
//! file. The job names and kinds come from `etc.inc` (`JobInfo`, WP-DATA-10).

use crate::types::JobId;
use flyff_common::defs::job;
use serde::{Deserialize, Serialize};

/// `MAX_JOB`.
pub const MAX_JOB: usize = job::MAX_JOB as usize;

/// The 18 float factors of one job, in file-column order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct JobProp {
    /// "ATK. Speed".
    pub attack_speed: f32,
    /// "MaxHP".
    pub factor_max_hp: f32,
    /// "MaxMP".
    pub factor_max_mp: f32,
    /// "MaxFP".
    pub factor_max_fp: f32,
    /// "Def".
    pub factor_def: f32,
    /// "HP Regen".
    pub factor_hp_recovery: f32,
    /// "MP Regen".
    pub factor_mp_recovery: f32,
    /// "FP Regen".
    pub factor_fp_recovery: f32,
    /// "SWD".
    pub melee_swd: f32,
    /// "AXE".
    pub melee_axe: f32,
    /// "STAFF".
    pub melee_staff: f32,
    /// "STICK".
    pub melee_stick: f32,
    /// "KNUCKLE".
    pub melee_knuckle: f32,
    /// "WAND".
    pub magic_wand: f32,
    /// "YOYO".
    pub melee_yoyo: f32,
    /// "BOW" (custom column).
    pub melee_bow: f32,
    /// "BLOCK".
    pub blocking: f32,
    /// "CRITICAL".
    pub critical: f32,
}

/// `m_aJob[job]` of `etc.inc`: name, base job and job type (`JTYPE_*`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    /// Display name (IDS key resolved).
    pub name: String,
    /// `dwJobBase`: the job this one advances from (a `JOB_*`).
    pub job_base: u32,
    /// `dwJobType`: `JTYPE_BASE/EXPERT/PRO/MASTER/HERO`.
    pub job_type: u32,
}

/// `m_aPropJob[32]` and `m_aJob[32]`; `None` for jobs the file does not list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobTable {
    /// `propJob.inc` rows by job id.
    pub props: Vec<Option<JobProp>>,
    /// `etc.inc` `job` block rows by job id.
    pub info: Vec<Option<JobInfo>>,
}

impl Default for JobTable {
    fn default() -> Self {
        Self {
            props: vec![None; MAX_JOB],
            info: vec![None; MAX_JOB],
        }
    }
}

impl JobTable {
    /// `GetJobProp(job)`: `None` for an unknown job (the original logs an Error).
    pub fn prop(&self, job: JobId) -> Option<&JobProp> {
        self.props.get(usize::try_from(job).ok()?)?.as_ref()
    }

    /// The `etc.inc` row of `job`.
    pub fn info(&self, job: JobId) -> Option<&JobInfo> {
        self.info.get(usize::try_from(job).ok()?)?.as_ref()
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_job_file;

#[cfg(feature = "load")]
mod parse {
    use super::{JobProp, MAX_JOB};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The rows of `propJob.inc` in file order.
    #[derive(Debug, Default)]
    pub(crate) struct JobFile {
        /// `(job, factors)`; a repeated job replaces the earlier row when applied.
        pub rows: Vec<(u32, JobProp)>,
    }

    /// `CProject::LoadPropJob` over decoded `text`. A job id `>= MAX_JOB` wrote out of bounds in the
    /// original: here it is an Error `IndexOutOfRange` and the row is skipped (its 18 floats are read).
    pub(crate) fn parse_job_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> JobFile {
        let mut out = JobFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        loop {
            let job = sc.get_number();
            if sc.finished() {
                break;
            }
            let p = JobProp {
                attack_speed: sc.get_float(),
                factor_max_hp: sc.get_float(),
                factor_max_mp: sc.get_float(),
                factor_max_fp: sc.get_float(),
                factor_def: sc.get_float(),
                factor_hp_recovery: sc.get_float(),
                factor_mp_recovery: sc.get_float(),
                factor_fp_recovery: sc.get_float(),
                melee_swd: sc.get_float(),
                melee_axe: sc.get_float(),
                melee_staff: sc.get_float(),
                melee_stick: sc.get_float(),
                melee_knuckle: sc.get_float(),
                magic_wand: sc.get_float(),
                melee_yoyo: sc.get_float(),
                melee_bow: sc.get_float(),
                blocking: sc.get_float(),
                critical: sc.get_float(),
            };
            if job as usize >= MAX_JOB {
                let msg = format!("{file}: job {job} is out of range (MAX_JOB {MAX_JOB})");
                sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                continue;
            }
            out.rows.push((job, p));
        }
        out
    }
}
