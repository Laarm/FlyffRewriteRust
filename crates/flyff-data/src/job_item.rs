//! `jobItem.inc` (`LoadJobItem`, Plan/14 §6.6, ref/17 §8.4; src:_Common/ProjectCmn.cpp:67-110).
//!
//! ```text
//! { 'job' JOB '{' { 'sex' SEX '{' { ITEM }* '}'  |  'item' ITYPE ITEM NUM }* '}' }*
//! ```
//!
//! `m_jobItem[job].adwMale[i][sex]` is the starting equipment of both sexes (up to `MAX_BEGINEQUIP` = 10
//! per sex); `beginItem[j] = {type, id, num}` are the starting inventory items (`MAX_BEGINITEM` = 42). The
//! arrays are pre-filled with `NULL_ID` (the original `memset(0xff)`). `MAX_JOBITEM` = 10 jobs.

use crate::types::{ItemId, JobId, NULL_ID};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `MAX_BEGINEQUIP`.
pub const MAX_BEGINEQUIP: usize = 10;
/// `MAX_BEGINITEM`.
pub const MAX_BEGINITEM: usize = 42;
/// `MAX_JOBITEM`.
pub const MAX_JOBITEM: u32 = 10;

/// One starting inventory item (`BEGINITEM`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginItem {
    /// `dwItemType` (`ITYPE_ITEM`, `ITYPE_CARD`, `ITYPE_CUBE`).
    pub item_type: u32,
    /// `dwItemId`.
    pub item: ItemId,
    /// `dwNum`.
    pub num: u32,
}

/// The starting equipment and items of one job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobItems {
    /// `adwMale[i][sex]` (`[slot][SEX_MALE = 0, SEX_FEMALE = 1]`), `NULL_ID` when unset.
    pub equip: [[ItemId; 2]; MAX_BEGINEQUIP],
    /// The `item` lines in file order (at most 42).
    pub begin_items: Vec<BeginItem>,
}

impl Default for JobItems {
    fn default() -> Self {
        Self {
            equip: [[NULL_ID; 2]; MAX_BEGINEQUIP],
            begin_items: Vec::new(),
        }
    }
}

/// `m_jobItem[]`, keyed by job id (only the jobs the file lists).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobItemTable {
    /// Jobs by id.
    pub jobs: BTreeMap<JobId, JobItems>,
}

impl JobItemTable {
    /// The entry of `job`.
    pub fn get(&self, job: JobId) -> Option<&JobItems> {
        self.jobs.get(&job)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_job_item;

#[cfg(feature = "load")]
mod parse {
    use super::{BeginItem, JobItemTable, JobItems, MAX_BEGINITEM, MAX_JOBITEM};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// `CProject::LoadJobItem` over decoded `text`. Out-of-range jobs/slots are an Error
    /// (`IndexOutOfRange`) and ignored; a truncated file ends the loops.
    pub(crate) fn parse_job_item(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> JobItemTable {
        let mut table = JobItemTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            if sc.is("job") {
                let job = sc.get_number();
                sc.get_token(); // {
                sc.get_token(); // sex
                let mut j = 0usize;
                let valid_job = job < MAX_JOBITEM;
                if !valid_job {
                    let msg = format!("jobItem: job {job} >= MAX_JOBITEM ({MAX_JOBITEM}); ignored");
                    sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                }
                let mut items = JobItems::default();
                while !sc.finished() && !sc.is("}") {
                    if sc.is("sex") {
                        let sex = sc.get_number();
                        sc.get_token(); // {
                        let mut i = 0usize;
                        let mut item = sc.get_number();
                        while !sc.finished() && !sc.is("}") {
                            let slot = usize::try_from(sex).ok().and_then(|s| {
                                items.equip.get_mut(i).and_then(|row| row.get_mut(s))
                            });
                            match slot {
                                Some(cell) => *cell = item,
                                None => {
                                    let msg = format!(
                                        "jobItem: equipment slot {i} / sex {sex} is out of range"
                                    );
                                    sc.report(Severity::Error, DiagCode::IndexOutOfRange, msg);
                                }
                            }
                            item = sc.get_number();
                            i += 1;
                        }
                    }
                    if sc.is("item") {
                        let b = BeginItem {
                            item_type: sc.get_number(),
                            item: sc.get_number(),
                            num: sc.get_number(),
                        };
                        if j < MAX_BEGINITEM {
                            items.begin_items.push(b);
                        } else {
                            let msg =
                                format!("jobItem: more than {MAX_BEGINITEM} begin items; ignored");
                            sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
                        }
                        j += 1;
                    }
                    sc.get_token();
                }
                if valid_job {
                    table.jobs.insert(job, items);
                }
            }
            sc.get_token();
        }
        table
    }
}
