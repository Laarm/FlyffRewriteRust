//! `MotionProp` and `LoadMotionProp` (Plan/14 §6.6, ref/17 §7.5; src:_Common/Project.cpp:985-1031).
//!
//! `propMotion.txt` is UTF-16, 27 rows of 7 words (9 tokens: the icon is three tokens). These are the
//! emote/action icons of the motion window. `dwAction`, `dwRequireLv`, `dwRequireExp`, `szRoot`, `szLink`
//! and `pTexture` are not loaded.

use crate::types::{EmoteId, MotionId};
use serde::{Deserialize, Serialize};

/// One row of `propMotion.txt`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MotionProp {
    /// word 0: version (rows with `ver <= 15` are kept).
    pub ver: i32,
    /// word 1: `MOT_*` (`id <= 0` is an Error and aborts the file).
    pub id: EmoteId,
    /// word 2: `MTI_*` animation id.
    pub motion: MotionId,
    /// word 3: `"""mot_baseSit.dds"""` (three tokens, the middle one is kept).
    pub icon: String,
    /// word 4: 0 = play once, 1 = continue, 2 = loop.
    pub play: u32,
    /// word 5: name (IDS key resolved).
    pub name: String,
    /// word 6: description (IDS key resolved).
    pub desc: String,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_motion_file;

#[cfg(feature = "load")]
mod parse {
    use super::MotionProp;
    use crate::props::item::DATA_VER;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The rows of `propMotion.txt` in file order.
    #[derive(Debug, Default)]
    pub(crate) struct MotionFile {
        /// Rows with `ver <= 15`.
        pub rows: Vec<MotionProp>,
        /// `id == 0` ended the file early.
        pub stopped_at_zero_id: bool,
    }

    /// `CProject::LoadMotionProp` over decoded `text`.
    pub(crate) fn parse_motion_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> MotionFile {
        let mut out = MotionFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut ver = sc.get_number_i32();
        let mut last = (0u32, String::new());
        while !sc.finished() {
            let id = sc.get_number();
            if id == 0 {
                let msg = format!("{file} ID:{id} last_read(ID:{} name:{})", last.0, last.1);
                sc.report(Severity::Error, DiagCode::PropIdInvalid, msg);
                out.stopped_at_zero_id = true;
                break;
            }
            let motion = sc.get_number();
            sc.get_token();
            sc.get_token();
            let icon = sc.token().to_owned();
            sc.get_token();
            let play = sc.get_number();
            sc.get_token();
            let name = sc.token().to_owned();
            sc.get_token();
            let desc = sc.token().to_owned();
            last = (id, name.clone());
            if ver <= DATA_VER {
                out.rows.push(MotionProp {
                    ver,
                    id,
                    motion,
                    icon,
                    play,
                    name,
                    desc,
                });
            }
            ver = sc.get_number_i32();
        }
        out
    }
}
