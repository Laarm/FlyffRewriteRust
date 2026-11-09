//! `World.inc`: the world id to folder list (`CWorldMng::LoadScript`, Plan/14 §6.9;
//! src:_Common/worldmng.cpp:302-379; ref/19 §4.1).
//!
//! ```text
//! WI_WORLD_MADRIGAL "WdMadrigal"                          // <id define> "<folder>"
//! WI_WORLD_MADRIGAL SetTitle ( IDS_WORLD_INC_000001 ) ;   // display name via the string table
//! ```
//!
//! 57 entries (56 titles) in this data set; several ids share one folder (`WI_DUNGEON_SECRET_1..16` →
//! `WdHeaven06_1`, `WI_WORLD_GUILDWAR1TO1_0..3` → `WdGuildWar1To1`). The scanner is the substituting
//! `CScript`, so the id is a define name that resolves to its number; the define name is kept for the
//! registry. A repeated id replaces the earlier entry (`SetAtGrow`, last wins); a `SetTitle` for an unknown
//! id is an Error (`WorldTitleUnknown`).

use crate::types::WorldId;
use serde::{Deserialize, Serialize};

/// One `World.inc` entry (`WORLD`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldDef {
    /// The world id (`WI_*` value).
    pub id: WorldId,
    /// The `WI_*` name as written in `World.inc` (the number when the file uses a literal).
    pub define_name: String,
    /// The folder as written (`m_szFileName`), e.g. `WdMadrigal`.
    pub folder: String,
    /// `SetTitle(IDS)`: the display name (`" "` for an empty text, empty when there is no `SetTitle`).
    pub title: String,
    /// `true` when the folder exists and has a `.wld` (set by the world-data step; the world server only
    /// loads these, Plan/15 §3.1).
    pub loadable: bool,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_world_inc;

#[cfg(feature = "load")]
mod parse {
    use super::WorldDef;
    use crate::types::WorldId;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// Reads one world id with `GetNumber` and also returns the define it came from.
    fn read_id(sc: &mut Script<'_>) -> (WorldId, Option<String>) {
        let mark = sc.mark();
        let (_, key) = sc.get_token_with_key();
        sc.reset(mark);
        (sc.get_number(), key)
    }

    /// `CWorldMng::LoadScript` over decoded `text`. A `}` or the end of the file ends the list, as in the
    /// original loop (`nBrace` counts only down).
    pub(crate) fn parse_world_inc(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> BTreeMap<WorldId, WorldDef> {
        let mut worlds: BTreeMap<WorldId, WorldDef> = BTreeMap::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let (mut id, mut key) = read_id(&mut sc);
        while !sc.token().starts_with('}') && !sc.finished() {
            sc.get_token(); // SetTitle or the folder
            if sc.is("SetTitle") {
                sc.get_token(); // (
                let title = sc.lang_script();
                if let Some(w) = worlds.get_mut(&id) {
                    w.title = title;
                } else {
                    sc.report(
                        Severity::Error,
                        DiagCode::WorldTitleUnknown,
                        format!("SetTitle for unknown world id {id}"),
                    );
                }
            } else {
                let folder = sc.token().to_owned();
                if folder.is_empty() {
                    sc.report(
                        Severity::Warning,
                        DiagCode::ValueOutOfRange,
                        format!("world id {id} has no folder name; ignored"),
                    );
                } else {
                    let def = WorldDef {
                        id,
                        define_name: key.take().unwrap_or_else(|| id.to_string()),
                        folder,
                        title: String::new(),
                        loadable: false,
                    };
                    if worlds.insert(id, def).is_some() {
                        sc.report(
                            Severity::Warning,
                            DiagCode::PropIdDuplicate,
                            format!("world id {id} is defined more than once; the last entry wins"),
                        );
                    }
                }
            }
            (id, key) = read_id(&mut sc);
        }
        worlds
    }
}
