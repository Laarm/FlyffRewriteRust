//! `propEnchant.inc` (`LoadPropEnchant`, Plan/14 §6.11, ref/17 §8.5; src:_Common/Project.cpp:4890-4951).
//!
//! `NORMAL NUM`, `AL NUM`, `NONE NUM`, `NORMAL_SCAL FLT x10`, `ALL_SCAL FLT x10`: the visual enchant
//! effect limits (`m_nEnchantLimitLevel[3]`) and scales (`m_fEnchantLevelScal[2][10]`).

use serde::{Deserialize, Serialize};

/// The enchant effect table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnchantTable {
    /// `NORMAL`, `AL`, `NONE` limit levels.
    pub limit_level: [i32; 3],
    /// `NORMAL_SCAL` (row 0) and `ALL_SCAL` (row 1): ten scales each.
    pub level_scale: [[f32; 10]; 2],
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_enchant;

#[cfg(feature = "load")]
mod parse {
    use super::EnchantTable;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `CProject::LoadPropEnchant` over decoded `text`.
    pub(crate) fn parse_enchant(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> EnchantTable {
        let mut t = EnchantTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            let key = sc.token().to_owned();
            match key.as_str() {
                "NORMAL" => t.limit_level[0] = sc.get_number_i32(),
                "AL" => t.limit_level[1] = sc.get_number_i32(),
                "NONE" => t.limit_level[2] = sc.get_number_i32(),
                "NORMAL_SCAL" | "ALL_SCAL" => {
                    let row = usize::from(key == "ALL_SCAL");
                    if let Some(scales) = t.level_scale.get_mut(row) {
                        for s in scales.iter_mut() {
                            *s = sc.get_float();
                        }
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        t
    }
}
