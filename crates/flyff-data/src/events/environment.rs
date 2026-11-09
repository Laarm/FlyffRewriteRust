//! `EnvironmentEffect.txt`: the season of each month by language (`CEnvironment::LoadScript`, Plan/14 §6.14; ref/19
//! §21; src:_Common/Environment.cpp:68-103).
//!
//! ```text
//! LANG M1 M2 ... M12            one row per language; each month is a SEASON_* (0..=4)
//! ```
//!
//! This data set's file is the single byte `0`: one row for `LANG_KOR` with twelve zeros (the number reads at
//! the end of the file give 0). The server language is `LANG_ENG`, which has no row, so there are no seasons
//! and [`EnvironmentTable::season`] is always `SEASON_NONE`.

use serde::{Deserialize, Serialize};

/// `LANG_MAX`.
pub const LANG_MAX: i32 = 13;
/// `SEASON_MAX`.
pub const SEASON_MAX: i32 = 5;

/// The environment-effect table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentTable {
    /// The rows in file order: `(language, [season of month 1..=12])`. A language that appears twice is
    /// overwritten by its later row in [`EnvironmentTable::seasons`].
    pub rows: Vec<(i32, [i32; 12])>,
    /// The twelve seasons of the server language (`LANG_ENG`); `None` when the file has no `LANG_ENG` row.
    pub seasons: Option<[i32; 12]>,
}

impl EnvironmentTable {
    /// `GetSeason()` for a month `1..=12` of the server language: the table value, `SEASON_NONE` (0) without a row.
    pub fn season(&self, month: u32) -> i32 {
        usize::try_from(month.wrapping_sub(1))
            .ok()
            .and_then(|m| self.seasons.as_ref().and_then(|s| s.get(m)))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_environment;

#[cfg(feature = "load")]
mod parse {
    use super::{EnvironmentTable, LANG_MAX, SEASON_MAX};
    use crate::events::generic_event::LANG_ENG;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// `CEnvironment::LoadScript` over decoded `text`. A language or season out of range stops the read (the
    /// original returns FALSE) with an Error; the rows read so far are kept.
    pub(crate) fn parse_environment(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> EnvironmentTable {
        let mut t = EnvironmentTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut lang = sc.get_number_i32();
        while !sc.finished() {
            if !(0..LANG_MAX).contains(&lang) {
                sc.report(
                    Severity::Error,
                    DiagCode::ValueOutOfRange,
                    format!("EnvironmentEffect.txt: language {lang} is outside 0..{LANG_MAX}"),
                );
                break;
            }
            let mut months = [0_i32; 12];
            for slot in &mut months {
                let season = sc.get_number_i32();
                if !(0..SEASON_MAX).contains(&season) {
                    sc.report(
                        Severity::Error,
                        DiagCode::ValueOutOfRange,
                        format!(
                            "EnvironmentEffect.txt: season {season} is outside 0..{SEASON_MAX}"
                        ),
                    );
                    return t;
                }
                *slot = season;
            }
            if u32::try_from(lang) == Ok(LANG_ENG) {
                t.seasons = Some(months);
            }
            t.rows.push((lang, months));
            lang = sc.get_number_i32();
        }
        t
    }
}
