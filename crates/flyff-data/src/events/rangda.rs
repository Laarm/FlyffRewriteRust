//! `randomeventmonster.inc`: the random-event ("Rangda") monsters (`CRangdaController::LoadScript`, Plan/14
//! §6.14; ref/15 §8; src:WORLDSERVER/rangda.cpp:187-228).
//!
//! ```text
//! MI '{' { nInterval N | nReplace N | bActiveAttack N | vRangda '{' (WORLD X Y Z)* '}' }* '}'
//! ```
//!
//! This data set has four monsters (`MI_RANGDA01..04`), three positions each (Madrigal, and the volcano
//! dungeon for the fourth), 3,600 s intervals and no first strike.

use serde::{Deserialize, Serialize};

/// One Rangda definition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RangdaDef {
    /// The monster (`MI_*`).
    pub mover: u32,
    /// Seconds between two respawns (`nInterval`; 0 when the block has none).
    pub interval_s: i32,
    /// Seconds between two position changes (`nReplace`).
    pub replace_s: i32,
    /// Whether it attacks first (`bActiveAttack`).
    pub active_attack: bool,
    /// The places it can appear: `(world, [x, y, z])`.
    pub points: Vec<(u32, [f32; 3])>,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_rangda;

#[cfg(feature = "load")]
mod parse {
    use super::RangdaDef;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CRangdaController::LoadScript` over decoded `text`.
    pub(crate) fn parse_rangda(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<RangdaDef> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut mover = sc.get_number();
        while !sc.finished() {
            let mut def = RangdaDef {
                mover,
                ..RangdaDef::default()
            };
            sc.get_token(); // {
            sc.get_token(); // subject or '}'
            while open(&sc) {
                match sc.token().to_owned().as_str() {
                    "nInterval" => def.interval_s = sc.get_number_i32(),
                    "nReplace" => def.replace_s = sc.get_number_i32(),
                    "bActiveAttack" => def.active_attack = sc.get_number() != 0,
                    "vRangda" => {
                        sc.get_token(); // {
                        let mut world = sc.get_number();
                        while open(&sc) {
                            let pos = [sc.get_float(), sc.get_float(), sc.get_float()];
                            def.points.push((world, pos));
                            world = sc.get_number();
                        }
                    }
                    _ => {}
                }
                sc.get_token();
            }
            out.push(def);
            mover = sc.get_number();
        }
        out
    }
}
