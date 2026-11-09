//! `Constant.inc` (`LoadConstant`/`ReadConstant`, Plan/14 §6.11, ref/17 §8.5;
//! src:_Common/Project.cpp:835-909).
//!
//! ```text
//! { 'lang' LANG '{' { 'formula' '{' { KEY '=' VALUE }* '}' | OTHER_BLOCK }* '}' }*
//! KEY: itemDropRate | monsterExpRate | monsterHitRate | goldDropRate (FLT), dwVagSP | dwExpertSP | dwProSP (NUM)
//! ```
//!
//! Only the block whose `LANG` equals the server language (`LANG_ENG`) is applied. **This data set has
//! no `LANG_ENG` block**, so the defaults hold: rates 1.0, skill-point costs Vag 1 / Expert 2 / Pro 3
//! (src:_Common/Project.cpp:128-136). The file is also reloaded at runtime by `/rec` in the original;
//! the rewrite loads it once (14 §13 item 7).

use serde::{Deserialize, Serialize};

/// The values of `Constant.inc`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConstantTable {
    /// The `lang` number of the block that was applied (`None`: no block matched).
    pub applied_lang_block: Option<i32>,
    /// `itemDropRate` (default 1.0).
    pub item_drop_rate: f32,
    /// `monsterExpRate` (default 1.0).
    pub monster_exp_rate: f32,
    /// `monsterHitRate` (default 1.0).
    pub monster_hit_rate: f32,
    /// `goldDropRate` (default 1.0).
    pub gold_drop_rate: f32,
    /// `dwVagSP` (default 1): skill points per level of a vagrant skill.
    pub vag_sp: u32,
    /// `dwExpertSP` (default 2).
    pub expert_sp: u32,
    /// `dwProSP` (default 3).
    pub pro_sp: u32,
}

impl Default for ConstantTable {
    fn default() -> Self {
        Self {
            applied_lang_block: None,
            item_drop_rate: 1.0,
            monster_exp_rate: 1.0,
            monster_hit_rate: 1.0,
            gold_drop_rate: 1.0,
            vag_sp: 1,
            expert_sp: 2,
            pro_sp: 3,
        }
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_constant;

#[cfg(feature = "load")]
mod parse {
    use super::ConstantTable;
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `CProject::LoadConstant` for the server language `lang` (`LANG_ENG` = 1).
    pub(crate) fn parse_constant(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        lang: i32,
        diags: &mut Diagnostics,
    ) -> ConstantTable {
        let mut t = ConstantTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        // The original's first iteration tests the (empty) initial token; the first real token is read at
        // the bottom of the loop.
        loop {
            if sc.is("lang") {
                let n = sc.get_number_i32();
                if n == lang {
                    t.applied_lang_block = Some(n);
                    sc.get_token(); // {
                    loop {
                        if sc.is("formula") {
                            read_constant(&mut sc, &mut t);
                        }
                        sc.get_token(); // server type
                        if sc.finished() || sc.token().starts_with('}') {
                            break;
                        }
                    }
                    sc.get_token(); // }
                }
            }
            sc.get_token();
            if sc.finished() {
                break;
            }
        }
        t
    }

    /// `CProject::ReadConstant`.
    fn read_constant(sc: &mut Script<'_>, t: &mut ConstantTable) {
        loop {
            sc.get_token();
            let key = sc.token().to_owned();
            match key.as_str() {
                "itemDropRate" => {
                    sc.get_token(); // =
                    t.item_drop_rate = sc.get_float();
                }
                "monsterExpRate" => {
                    sc.get_token();
                    t.monster_exp_rate = sc.get_float();
                }
                "monsterHitRate" => {
                    sc.get_token();
                    t.monster_hit_rate = sc.get_float();
                }
                "goldDropRate" => {
                    sc.get_token();
                    t.gold_drop_rate = sc.get_float();
                }
                "dwVagSP" => {
                    sc.get_token();
                    t.vag_sp = sc.get_number();
                }
                "dwExpertSP" => {
                    sc.get_token();
                    t.expert_sp = sc.get_number();
                }
                "dwProSP" => {
                    sc.get_token();
                    t.pro_sp = sc.get_number();
                }
                _ => {}
            }
            if sc.finished() || sc.token().starts_with('}') {
                break;
            }
        }
    }
}
