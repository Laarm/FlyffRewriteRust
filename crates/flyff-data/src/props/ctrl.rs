//! `CtrlProp` and `LoadPropCtrl` (Plan/14 §6.6, ref/17 §7.4; src:_Common/Project.cpp:4997-5038).
//!
//! `propCtrl.txt` is UTF-16, 78 rows of 8 tokens, no version column. Rows are stored in `m_aPropCtrl[id]`
//! and `m_mapCtrl` (display name to id, first wins).

use crate::types::CtrlId;
use serde::{Deserialize, Serialize};

/// One row of `propCtrl.txt`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CtrlProp {
    /// t0: `CI_*`.
    pub id: CtrlId,
    /// t1: display name (IDS key resolved).
    pub name: String,
    /// t2: `CK1_*` (chest, trigger, guild house, housing, ...).
    pub ctrl_kind1: u32,
    /// t3: `CK2_*`.
    pub ctrl_kind2: u32,
    /// t4: `CK3_*`.
    pub ctrl_kind3: u32,
    /// t5.
    pub sfx_ctrl: u32,
    /// t6.
    pub snd_damage: u32,
    // t7 szComment: read and discarded
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_ctrl_file;

#[cfg(feature = "load")]
mod parse {
    use super::CtrlProp;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The rows of `propCtrl.txt` in file order.
    #[derive(Debug, Default)]
    pub(crate) struct CtrlFile {
        /// Rows with a non-negative id (a zero id is reported but stored, as `SetAtGrow(0)` did).
        pub rows: Vec<CtrlProp>,
    }

    /// `CProject::LoadPropCtrl` over decoded `text`. `id <= 0` is an Error and the loop continues.
    pub(crate) fn parse_ctrl_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> CtrlFile {
        let mut out = CtrlFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut last = String::new();
        let mut id = sc.get_number_i32();
        while !sc.finished() {
            if id <= 0 {
                let msg = format!("{file}: ID {id} read after '{last}'");
                sc.report(Severity::Error, DiagCode::PropIdInvalid, msg);
            }
            sc.get_token();
            let name = sc.token().to_owned();
            last.clone_from(&name);
            let p = CtrlProp {
                id: id as u32,
                name,
                ctrl_kind1: sc.get_number(),
                ctrl_kind2: sc.get_number(),
                ctrl_kind3: sc.get_number(),
                sfx_ctrl: sc.get_number(),
                snd_damage: sc.get_number(),
            };
            sc.get_token(); // szComment: skipped
            if id >= 0 {
                out.rows.push(p);
            }
            id = sc.get_number_i32();
        }
        out
    }
}
