//! `Client/sound.inc` and `Music/default.bgm`: the id to file tables of the sound and music managers
//! (`CSoundManager::LoadScript`, `CMusicMng::LoadScript`; Plan/14 §6.19, ref/20 §13.2, §13.6;
//! src:_DirectX/dsutil.cpp:41-59, _Common/Music2.cpp:195-213).
//!
//! Both files are the same grammar, `{ ID "file" }*`, read with `CScript` (the id is a define such as
//! `SND_INF_CLICK` or `BGM_TITLE`; a later entry for the same id replaces an earlier one, like
//! `CFixedArray::SetAtGrow`). The value is kept as written (`"music\BgmDu1Mystery.ogg"`, `"InfClick.wav"`,
//! `""` = silent); the bundle exporter turns it into a canonical VFS path.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An id to file-name table (`m_aMusic`), plus the number of entries the file listed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdFileTable {
    /// id to the file name as written (`""` = silent), last definition wins.
    pub files: BTreeMap<u32, String>,
    /// Entries in the file (duplicates counted: 1,667 for `sound.inc`, 42 for `default.bgm`).
    pub entries: u32,
    /// Entries with an empty file name, overridden ones included (179 for `sound.inc`; one of them,
    /// `BGM_NONE ""`, is replaced by `SND_NONE "InfClick.wav"` of the same id 0, leaving 178 silent ids).
    pub empty_entries: u32,
}

impl IdFileTable {
    /// The file written for `id`.
    pub fn get(&self, id: u32) -> Option<&str> {
        self.files.get(&id).map(String::as_str)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_id_file;

#[cfg(feature = "load")]
mod parse {
    use super::IdFileTable;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The loop of both loaders over decoded `text`.
    ///
    /// The original is a `do { ... } while (tok != FINISHED)` that would store a phantom entry (id 0, empty
    /// name) for a file without any entry; an empty or comment-only file gives an empty table here
    /// (**FIX**, no effect on the data set). A truncated last entry (id without a file) is dropped.
    pub(crate) fn parse_id_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> IdFileTable {
        let mut out = IdFileTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut id = sc.get_number();
        while !sc.finished() {
            sc.get_token(); // the file name
            if sc.finished() {
                sc.report(
                    Severity::Warning,
                    DiagCode::ValueOutOfRange,
                    format!("id {id} has no file name (truncated file)"),
                );
                break;
            }
            out.entries = out.entries.saturating_add(1);
            if sc.token().is_empty() {
                out.empty_entries = out.empty_entries.saturating_add(1);
            }
            out.files.insert(id, sc.token().to_owned());
            id = sc.get_number();
        }
        out
    }
}
