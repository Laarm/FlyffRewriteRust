//! `DiePenalty.inc` (`LoadScriptDiePenalty`, Plan/14 §6.11, ref/17 §8.5; src:_Common/Project.cpp:5040-5110).
//!
//! ```text
//! { ('REVIVAL_PENALTY' | 'DECEXP_PENALTY' | 'LEVEL_DOWN_PENALTY') '{' { 'ADDPENALTY' LEVEL VALUE }* '}' }*
//! ```
//!
//! Stored as `(level, value)` in file order. Percentages are x100 (8000 = 80 %); the level-down table
//! holds 0/1. The opening usage `/* */` block of the file is a comment.

use serde::{Deserialize, Serialize};

/// The three penalty tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiePenaltyTable {
    /// `REVIVAL_PENALTY`: `(level, hp recovery percent x100)`.
    pub revival: Vec<(i32, i32)>,
    /// `DECEXP_PENALTY`: `(level, exp loss percent x100)`.
    pub dec_exp: Vec<(i32, i32)>,
    /// `LEVEL_DOWN_PENALTY`: `(level, may level down 0/1)`.
    pub level_down: Vec<(i32, i32)>,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_die_penalty;

#[cfg(feature = "load")]
mod parse {
    use super::DiePenaltyTable;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `CProject::LoadScriptDiePenalty` over decoded `text`.
    pub(crate) fn parse_die_penalty(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> DiePenaltyTable {
        let mut t = DiePenaltyTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            // The three `if`s of the original test the current token one after another.
            for which in 0..3 {
                let name = match which {
                    0 => "REVIVAL_PENALTY",
                    1 => "DECEXP_PENALTY",
                    _ => "LEVEL_DOWN_PENALTY",
                };
                if !sc.is(name) {
                    continue;
                }
                sc.get_token(); // {
                sc.get_token(); // first token of the block
                while !sc.finished() && !sc.token().starts_with('}') {
                    if sc.is("ADDPENALTY") {
                        let row = (sc.get_number_i32(), sc.get_number_i32());
                        match which {
                            0 => t.revival.push(row),
                            1 => t.dec_exp.push(row),
                            _ => t.level_down.push(row),
                        }
                    }
                    sc.get_token();
                }
            }
            sc.get_token();
        }
        t
    }
}
