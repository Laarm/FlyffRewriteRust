//! `etc.inc` (`LoadEtc`, Plan/14 §6.6, ref/17 §8.3; src:_Common/Project.cpp:936-983).
//!
//! UTF-16, three blocks:
//!
//! ```text
//! 'job'       '{' { JOB NAME_STR JOBBASE JTYPE }* '}'   -> m_aJob[job] = {name, job base, job type}
//! 'structure' '{' { SRT NAME_STR }* '}'                 -> m_aStructure[srt].szName
//! 'Guild'     '{' { GUD NAME_STR }* '}'                 -> m_aGuildAppell[gud].szName (MAX 5)
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `MAX_GUILDAPPELL`.
pub const MAX_GUILDAPPELL: usize = 5;

/// The names of `etc.inc` that are not jobs (`SRT_*` structures and `GUD_*` guild appellations).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EtcNames {
    /// `SRT_*` to name (e.g. `SRT_QUESTOFFICE`), at most `MAX_STRUCTURE` (20) ids.
    pub structures: BTreeMap<u32, String>,
    /// `GUD_*` to the guild rank name, at most [`MAX_GUILDAPPELL`] ids.
    pub guild_appellations: BTreeMap<u32, String>,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_etc;

#[cfg(feature = "load")]
mod parse {
    use super::{EtcNames, MAX_GUILDAPPELL};
    use crate::props::job::{JobInfo, MAX_JOB};
    use crate::types::JobId;
    use flyff_common::defs::neuz::MAX_STRUCTURE;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// What `LoadEtc` produces.
    #[derive(Debug, Default)]
    pub(crate) struct EtcFile {
        /// The `job` block rows `(job, info)` in file order.
        pub jobs: Vec<(JobId, JobInfo)>,
        /// The other two blocks.
        pub names: EtcNames,
    }

    fn open(sc: &Script<'_>) -> bool {
        !sc.finished() && !sc.token().starts_with('}')
    }

    /// `CProject::LoadEtc` over decoded `text`. Ids beyond the original arrays are an Error and ignored.
    pub(crate) fn parse_etc(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> EtcFile {
        let mut out = EtcFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        loop {
            sc.get_token();
            let key = sc.token().to_owned();
            match key.as_str() {
                "job" => {
                    sc.get_token(); // {
                    let mut id = sc.get_number();
                    while open(&sc) {
                        sc.get_token();
                        let info = JobInfo {
                            name: sc.token().to_owned(),
                            job_base: sc.get_number(),
                            job_type: sc.get_number(),
                        };
                        if (id as usize) < MAX_JOB {
                            out.jobs.push((id, info));
                        } else {
                            let msg = format!("etc.inc: job {id} >= MAX_JOB; ignored");
                            sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                        }
                        id = sc.get_number();
                    }
                }
                "structure" => {
                    sc.get_token(); // {
                    let mut id = sc.get_number();
                    while open(&sc) {
                        sc.get_token();
                        let name = sc.token().to_owned();
                        if id < MAX_STRUCTURE {
                            out.names.structures.insert(id, name);
                        } else {
                            let msg = format!("etc.inc: structure {id} >= MAX_STRUCTURE; ignored");
                            sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                        }
                        id = sc.get_number();
                    }
                }
                "Guild" => {
                    sc.get_token(); // {
                    let mut id = sc.get_number();
                    while open(&sc) {
                        sc.get_token();
                        let name = sc.token().to_owned();
                        if (id as usize) < MAX_GUILDAPPELL {
                            out.names.guild_appellations.insert(id, name);
                        } else {
                            let msg = format!(
                                "etc.inc: guild appellation {id} >= {MAX_GUILDAPPELL}; ignored"
                            );
                            sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                        }
                        id = sc.get_number();
                    }
                }
                _ => {}
            }
            if sc.finished() {
                break;
            }
        }
        out
    }
}
