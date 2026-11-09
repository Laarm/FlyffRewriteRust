//! `honorList.txt`: the honor titles (`CTitleManager::LoadTitle`, Plan/14 §6.15, ref/17 §8.11;
//! src:_Common/honor.cpp:34-98). Rows of `ID LGROUP SGROUP NEED TITLE`: the group names are defines
//! (`HI_COUNT_CHECK`, `HS_JUMP`, `JOB_KNIGHT_MASTER`, `MI_...`), the title an `IDS_TITLE_TXT_n` key. 58 rows
//! in the reference data.

use serde::{Deserialize, Serialize};

/// One honor title (`HonorData`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HonorTitle {
    /// The first column of the row.
    pub id: u32,
    /// `HI_*`: the kind of title (count check, earn title, hunt monster, use item, ...).
    pub large_group: u32,
    /// `HS_*`, `JOB_*` or `MI_*`/`II_*` depending on the large group.
    pub small_group: u32,
    /// The count (or level) needed.
    pub need: i32,
    /// The title text.
    pub title: String,
}

/// The honor titles in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HonorTable {
    /// The rows.
    pub titles: Vec<HonorTitle>,
}

impl HonorTable {
    /// The row with the id `id` (`CTitleManager::m_mapAll`; the first one when a file repeats an id).
    pub fn title(&self, id: u32) -> Option<&HonorTitle> {
        self.titles.iter().find(|t| t.id == id)
    }

    /// The row of the small group `small` inside the large group `large` (`GetHonorDataByID(small, large)`): BOTH
    /// must match, so `HS_STR` under `HI_COUNT_CHECK` is title 5 while the same number under another group is
    /// none. The first row wins (the original keeps the first insert of a duplicate key).
    pub fn title_by(&self, small: u32, large: u32) -> Option<&HonorTitle> {
        self.titles
            .iter()
            .find(|t| t.small_group == small && t.large_group == large)
    }

    /// The count a title needs (`GetNeedCount(id, -1)`); `None` for an id without a row.
    pub fn need_of(&self, id: u32) -> Option<i32> {
        self.title(id).map(|t| t.need)
    }
}

/// The most titles accepted (a guard; the data has 58).
pub const MAX_HONOR_TITLES: usize = 4096;

#[cfg(feature = "load")]
pub(crate) use parse::parse_honor;

#[cfg(feature = "load")]
mod parse {
    use super::{HonorTable, HonorTitle, MAX_HONOR_TITLES};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// `LoadTitle` over decoded `text`. A row that runs into the end of the file is dropped.
    pub(crate) fn parse_honor(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> HonorTable {
        let mut out = HonorTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut id = sc.get_number();
        while !sc.finished() {
            let large_group = sc.get_number();
            let small_group = sc.get_number();
            let need = sc.get_number_i32();
            sc.get_token();
            if sc.finished() {
                break;
            }
            if out.titles.len() >= MAX_HONOR_TITLES {
                sc.report(
                    Severity::Error,
                    DiagCode::CapacityExceeded,
                    format!(
                        "honorList.txt: more than {MAX_HONOR_TITLES} titles; the rest is ignored"
                    ),
                );
                break;
            }
            out.titles.push(HonorTitle {
                id,
                large_group,
                small_group,
                need,
                title: sc.token().to_owned(),
            });
            id = sc.get_number();
        }
        out
    }
}
