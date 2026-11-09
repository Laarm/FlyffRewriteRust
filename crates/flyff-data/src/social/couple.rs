//! `couple.inc`: couple levels, items and skills (`CCoupleProperty::Initialize`, Plan/14 §6.15, F12;
//! src:_Common/couple.cpp:234-343).
//!
//! ```text
//! Level      '{' EXP* '}'                                   exp per level; F12: each EXP / couple_rate
//! Item       '{' (LEVEL '{' (ITEM SEX FLAGS LIFE NUM)* '}')* '}'     (empty in this data set)
//! SkillKind  '{' ITEM* '}'
//! SkillLevel '{' (LV LEVEL_PER_KIND{kinds})* '}'
//! ```

use serde::{Deserialize, Serialize};

/// One couple item (`COUPLE_ITEM`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoupleItem {
    /// The item.
    pub item: u32,
    /// 0 male, 1 female, 2 sexless.
    pub sex: i32,
    /// Flags.
    pub flags: i32,
    /// Life.
    pub life: i32,
    /// Count.
    pub num: i32,
}

/// The couple tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoupleTable {
    /// EXP per level, already divided by the couple rate (F12, integer division).
    pub level_exp: Vec<i32>,
    /// The items granted per level (index = level - 1), same length as `level_exp`.
    pub items: Vec<Vec<CoupleItem>>,
    /// The couple skill items (`SkillKind`).
    pub skill_kinds: Vec<u32>,
    /// `SkillLevel` rows: the level and the level of each skill kind.
    pub skill_levels: Vec<(i32, Vec<i32>)>,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_couple;

#[cfg(feature = "load")]
mod parse {
    use super::{CoupleItem, CoupleTable};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CCoupleProperty::Initialize`. `exp_divisor` is `gameplay.couple_rate` (a value of 0 is treated as 1).
    pub(crate) fn parse_couple(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        exp_divisor: i32,
        diags: &mut Diagnostics,
    ) -> CoupleTable {
        let mut t = CoupleTable::default();
        let div = exp_divisor.max(1);
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "Level" => {
                    sc.get_token(); // {
                    let mut exp = sc.get_number_i32();
                    while open(&sc) {
                        t.level_exp.push(exp / div);
                        exp = sc.get_number_i32();
                    }
                    t.items.resize(t.level_exp.len(), Vec::new());
                }
                "Item" => {
                    sc.get_token(); // {
                    let mut level = sc.get_number_i32();
                    while open(&sc) {
                        sc.get_token(); // {
                        let mut item = sc.get_number();
                        while open(&sc) {
                            let ci = CoupleItem {
                                item,
                                sex: sc.get_number_i32(),
                                flags: sc.get_number_i32(),
                                life: sc.get_number_i32(),
                                num: sc.get_number_i32(),
                            };
                            let slot = usize::try_from(level.saturating_sub(1))
                                .ok()
                                .and_then(|i| t.items.get_mut(i));
                            match slot {
                                Some(v) => v.push(ci),
                                None => sc.report(
                                    Severity::Error,
                                    DiagCode::IndexOutOfRange,
                                    format!(
                                        "couple.inc: item level {level} is outside 1..={}",
                                        t.level_exp.len()
                                    ),
                                ),
                            }
                            item = sc.get_number();
                        }
                        level = sc.get_number_i32();
                    }
                }
                "SkillKind" => {
                    sc.get_token(); // {
                    let mut item = sc.get_number();
                    while open(&sc) {
                        t.skill_kinds.push(item);
                        item = sc.get_number();
                    }
                }
                "SkillLevel" => {
                    sc.get_token(); // {
                    let mut level = sc.get_number_i32();
                    while open(&sc) {
                        let mut per_kind = Vec::with_capacity(t.skill_kinds.len());
                        for _ in 0..t.skill_kinds.len() {
                            per_kind.push(sc.get_number_i32());
                        }
                        t.skill_levels.push((level, per_kind));
                        level = sc.get_number_i32();
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        t
    }
}
