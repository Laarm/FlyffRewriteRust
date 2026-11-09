//! `textClient.inc` and `GETTEXT` semantics (Plan/14 §4.5, ref/17 §8.1; src:_Common/ProjectCmn.cpp:558-606).
//!
//! Grammar: `{ TID COLOR '{' STR '}' }*` (2,975 active entries). Double quotes inside the string are
//! removed, a later entry for the same TID replaces an earlier one, and entries with empty text are
//! not stored. The table is used by the server too (system and notice messages are format strings with
//! `%s`/`%d` placeholders).

use crate::types::{IdTable, TextId};
use serde::{Deserialize, Serialize};

/// One text: the display string and its ARGB colour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEntry {
    /// The text with `"` removed.
    pub text: String,
    /// ARGB, e.g. `0xffffffff`.
    pub color: u32,
}

/// The text table (`m_colorText`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextTable {
    entries: IdTable<TextEntry>,
}

impl TextTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// `m_colorText.GetSize()`: highest stored (non-empty) TID + 1.
    pub fn size(&self) -> u32 {
        self.entries.size()
    }

    /// Number of stored (non-empty) texts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no text is stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `GETTEXT(tid)`: `"error"` when `tid` is out of range (`tid >= size`, or not a positive `int`),
    /// `""` for a hole.
    pub fn get(&self, tid: TextId) -> &str {
        if !self.in_range(tid) {
            return "error";
        }
        self.entries.get(tid).map_or("", |e| e.text.as_str())
    }

    /// `GETTEXTCOLOR(tid)`: `0xFFFFFFFF` when out of range, `0` for a hole.
    pub fn get_color(&self, tid: TextId) -> u32 {
        if !self.in_range(tid) {
            return 0xFFFF_FFFF;
        }
        self.entries.get(tid).map_or(0, |e| e.color)
    }

    /// The stored entry of `tid`, if any (no range semantics).
    pub fn entry(&self, tid: TextId) -> Option<&TextEntry> {
        self.entries.get(tid)
    }

    /// Stored `(tid, entry)` pairs in ascending TID order.
    pub fn iter(&self) -> impl Iterator<Item = (TextId, &TextEntry)> + '_ {
        self.entries.iter()
    }

    /// Stores a text (last wins). Empty texts are not stored, like the original; a TID above
    /// [`IdTable::MAX_ID`] is refused (`false`).
    pub fn set(&mut self, tid: TextId, text: &str, color: u32) -> bool {
        if text.is_empty() {
            return true;
        }
        self.entries
            .insert(
                tid,
                TextEntry {
                    text: text.to_owned(),
                    color,
                },
            )
            .is_ok()
    }

    // `VERIFY_RANGE((int)dwIndex, 0, GetSize(), ...)`
    fn in_range(&self, tid: TextId) -> bool {
        i32::try_from(tid).is_ok() && tid < self.entries.size()
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_text_table;

#[cfg(feature = "load")]
mod parse {
    use super::TextTable;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// The result of [`parse_text_table`].
    #[derive(Debug, Default)]
    pub(crate) struct ParsedTexts {
        /// The stored (non-empty) texts.
        pub table: TextTable,
        /// Distinct TIDs the file defines, including those whose text is empty (2,975 in the reference
        /// data; 10 of them are empty, so the table holds 2,965).
        pub entries: u32,
    }

    /// `CProject::LoadText` over decoded `text`.
    pub(crate) fn parse_text_table(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> ParsedTexts {
        // `SetAtGrow` per TID: the last entry wins; only non-empty texts survive into the table.
        let mut by_id: BTreeMap<u32, (String, u32)> = BTreeMap::new();
        {
            let mut sc = Script::new(
                text,
                Arc::clone(file),
                ScannerOptions::default(),
                sctx,
                diags,
            );
            let mut id = sc.get_number();
            loop {
                let color = sc.get_number();
                sc.get_token();
                if sc.token().starts_with('{') {
                    sc.get_token();
                    let s = sc.token().replace('"', "");
                    by_id.insert(id, (s, color));
                    sc.get_token(); // }
                }
                id = sc.get_number();
                if sc.finished() {
                    break;
                }
            }
        }
        let entries = u32::try_from(by_id.len()).unwrap_or(u32::MAX);
        let mut table = TextTable::new();
        for (id, (s, color)) in by_id {
            if !table.set(id, &s, color) {
                diags.report(
                    Severity::Error,
                    DiagCode::IndexOutOfRange,
                    Some(file),
                    0,
                    format!("text id {id} is out of range and ignored"),
                );
            }
        }
        ParsedTexts { table, entries }
    }
}
