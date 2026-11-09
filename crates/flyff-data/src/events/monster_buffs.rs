//! `MonsterBuffs.inc` (`CProject::LoadMonsterBuffs`, Plan/14 §6.14, ref/17 §8.7; src:_Common/Project.cpp:784-800)
//! and `DPSProp.txt` (`CProject::LoadDPSProp`, src:_Common/ProjectCmn.cpp:926-961).
//!
//! `MonsterBuffs.inc` is a flat list of `MOVER ITEM` pairs (127 in this data, all `II_SYS_SYS_EVE_WINGS`); the
//! loader inserts into a `std::map`, so the **first** pair of a mover wins. `DPSProp.txt` is
//! `MONSTER '{' id* '}'` with plain numeric ids (`atoi`); it is empty in this data.

use serde::{Deserialize, Serialize};

/// The pairs of `MonsterBuffs.inc` in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterBuffTable {
    /// `(mover id, item id)` as listed.
    pub pairs: Vec<(u32, u32)>,
}

impl MonsterBuffTable {
    /// The buff item of `mover`: the first pair that lists it (`std::map::insert` keeps the first).
    pub fn item_for(&self, mover: u32) -> Option<u32> {
        self.pairs
            .iter()
            .find(|(m, _)| *m == mover)
            .map(|(_, i)| *i)
    }

    /// Number of pairs.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// `true` when the table has no pair.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_dps_monsters, parse_monster_buffs};

#[cfg(feature = "load")]
mod parse {
    use super::MonsterBuffTable;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `LoadMonsterBuffs` over decoded `text`: `monsterID = GetNumber(); while not finished { buffID = GetNumber();
    /// insert; monsterID = GetNumber(); }`.
    pub(crate) fn parse_monster_buffs(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> MonsterBuffTable {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut pairs = Vec::new();
        let mut mover = sc.get_number();
        while !sc.finished() {
            let item = sc.get_number();
            pairs.push((mover, item));
            mover = sc.get_number();
        }
        MonsterBuffTable { pairs }
    }

    /// `LoadDPSProp`: every `MONSTER { id id ... }` block adds its ids (`atoi` of the token, so a symbolic name
    /// reads as 0, as in the original).
    pub(crate) fn parse_dps_monsters(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<u32> {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut out = Vec::new();
        sc.get_token();
        while !sc.finished() {
            if sc.token() == "MONSTER" {
                sc.get_token(); // {
                sc.get_token(); // } or an id
                while !sc.token().starts_with('}') && !sc.finished() {
                    out.push(
                        u32::try_from(flyff_script::c_atoi(sc.token().as_bytes())).unwrap_or(0),
                    );
                    sc.get_token();
                }
                // the original reads one more token here and another at the end of the loop, which skips the
                // `MONSTER` of a second block; one read keeps every block (a single block is unaffected)
                sc.get_token();
                continue;
            }
            sc.get_token();
        }
        out
    }
}
