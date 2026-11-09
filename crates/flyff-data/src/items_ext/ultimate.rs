//! The ultimate-weapon tables: `Ultimate_GemAbility.txt` (the abilities an ultimate weapon gets from its gems,
//! `CUltimateWeapon::Load_GemAbility`, src:_Common/UltimateWeapon.cpp:45-97) and `Ultimate_UltimateWeapon.txt`
//! (the server probabilities, `Load_UltimateWeapon`, `UltimateWeapon.cpp:116-181`; Plan/14 §6.13, ref/13
//! §18.8). Both are read with a raw `CScanner`; names are resolved with `GetDefineNum`.
//!
//! ```text
//! Ultimate_GemAbility.txt
//!   ABILITY DST '{' (GEM_ITEM ABILITY_ITEM V1 V2 V3 V4 V5)* '}'
//!   LEVEL_GEM '{' (GEM_ITEM MINLEVEL MAXLEVEL)* '}'
//! Ultimate_UltimateWeapon.txt                  (probabilities are n / 1,000,000)
//!   SET_GEM P   REMOVE_GEM P   GENERAL2UNIQUE P   UNIQUE2ULTIMATE P
//!   MAKE_GEM '{' (AO GENERAL_PROB GENERAL_NUM UNIQUE_PROB UNIQUE_NUM)* '}'
//!   ULTIMATE_ENCHANT '{' (TARGET_AO PROB)* '}'
//! ```
//!
//! The client shows the five values of each gem; the probabilities are server data.

use serde::{Deserialize, Serialize};

/// One gem of an ability.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GemAbilityRow {
    /// The gem item.
    pub gem: u32,
    /// The item that carries the ability (`dwAbilityItemId`).
    pub gem06: u32,
    /// The five ability values (one per ultimate level).
    pub values: Vec<i32>,
}

/// The gems of one destination parameter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GemAbilityDef {
    /// `DST_*`.
    pub dst: u32,
    /// The gem rows.
    pub rows: Vec<GemAbilityRow>,
}

/// The gem a weapon level range makes (`__MAKEGEMITEM`, one `LEVEL_GEM` line).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MakeGemItem {
    /// The gem item.
    pub item: u32,
    /// The lowest weapon level (`dwLimitLevel1`).
    pub min_level: i32,
    /// The highest weapon level.
    pub max_level: i32,
}

/// One `MAKE_GEM` row (`__MAKEGEMPROB`): what a destroyed weapon of the given upgrade level yields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MakeGemProb {
    /// The weapon's upgrade level (`nEnchant`).
    pub ability_option: i32,
    /// The probability for a general weapon (n / 1,000,000).
    pub general_prob: u32,
    /// The number of gems from a general weapon.
    pub general_num: i32,
    /// The probability for a unique weapon (n / 1,000,000).
    pub unique_prob: u32,
    /// The number of gems from a unique weapon.
    pub unique_num: i32,
}

/// The ultimate-weapon tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UltimateTables {
    /// `ABILITY` blocks in file order (`Ultimate_GemAbility.txt`).
    pub gem_ability: Vec<GemAbilityDef>,
    /// `LEVEL_GEM` rows: the gem a weapon level range makes (`Ultimate_GemAbility.txt`; formerly `make_gem`).
    pub level_gem: Vec<MakeGemItem>,
    /// `SET_GEM`: the probability that a gem is socketed (n / 1,000,000; 1,000,000).
    pub set_gem: u32,
    /// `REMOVE_GEM`: the probability that a gem is removed (1,000,000).
    pub remove_gem: u32,
    /// `GENERAL2UNIQUE`: general to unique (20,000 = 2 %).
    pub general2unique: u32,
    /// `UNIQUE2ULTIMATE`: unique to ultimate (1,000,000).
    pub unique2ultimate: u32,
    /// `MAKE_GEM` rows by upgrade level (the original `map<int, __MAKEGEMPROB>`: the first row of a level wins),
    /// in file order.
    pub make_gem: Vec<MakeGemProb>,
    /// `ULTIMATE_ENCHANT`: `(target upgrade level, probability n / 1,000,000)` in file order (first wins).
    pub enchant: Vec<(i32, i32)>,
}

impl UltimateTables {
    /// `GetGemKind( limitLevel )`: the gem of the first `LEVEL_GEM` range that contains the level.
    pub fn gem_kind(&self, limit_level: i32) -> Option<u32> {
        self.level_gem
            .iter()
            .find(|g| limit_level <= g.max_level && limit_level >= g.min_level)
            .map(|g| g.item)
    }

    /// The `MAKE_GEM` row of an upgrade level.
    pub fn make_gem_prob(&self, ability_option: i32) -> Option<&MakeGemProb> {
        self.make_gem
            .iter()
            .find(|m| m.ability_option == ability_option)
    }

    /// The `ULTIMATE_ENCHANT` probability of reaching `target_level`.
    pub fn enchant_prob(&self, target_level: i32) -> Option<i32> {
        self.enchant
            .iter()
            .find(|(l, _)| *l == target_level)
            .map(|(_, p)| *p)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_gem_ability, parse_ultimate_weapon};

#[cfg(feature = "load")]
mod parse {
    use super::{GemAbilityDef, GemAbilityRow, MakeGemItem, MakeGemProb, UltimateTables};
    use flyff_script::{
        DiagCode, Diagnostics, Scanner, ScannerOptions, ScriptCtx, Severity, c_atoi,
    };
    use std::sync::Arc;

    /// `Load_GemAbility` over decoded `text`.
    pub(crate) fn parse_gem_ability(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> UltimateTables {
        let mut out = UltimateTables::default();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        let define = |sc: &Scanner<'_>| sctx.defines.get(sc.token()).map_or(u32::MAX, |v| v as u32);
        let open = |sc: &Scanner<'_>| !sc.token().starts_with('}') && !sc.finished();
        sc.get_token(false);
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "ABILITY" => {
                    sc.get_token(false);
                    let mut def = GemAbilityDef {
                        dst: define(&sc),
                        rows: Vec::new(),
                    };
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while open(&sc) {
                        let gem = define(&sc);
                        sc.get_token(false);
                        let gem06 = define(&sc);
                        let values = (0..5).map(|_| sc.get_number_i32(false)).collect();
                        def.rows.push(GemAbilityRow { gem, gem06, values });
                        sc.get_token(false);
                    }
                    out.gem_ability.push(def);
                }
                "LEVEL_GEM" => {
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while open(&sc) {
                        let item = define(&sc);
                        if item == u32::MAX {
                            // the original `Error()`s and returns FALSE; the line is skipped here
                            let line = sc.line();
                            let msg = format!("LEVEL_GEM: {} is not a define", sc.token());
                            diags.report(
                                Severity::Error,
                                DiagCode::UnknownIdentifier,
                                Some(sc.file()),
                                line,
                                msg,
                            );
                        }
                        let row = MakeGemItem {
                            item,
                            min_level: sc.get_number_i32(false),
                            max_level: sc.get_number_i32(false),
                        };
                        if item != u32::MAX {
                            out.level_gem.push(row);
                        }
                        sc.get_token(false);
                    }
                }
                _ => {}
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        out
    }

    /// `Load_UltimateWeapon` over decoded `text`, merged into `out` (the gem abilities are kept).
    pub(crate) fn parse_ultimate_weapon(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
        out: &mut UltimateTables,
    ) {
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        let open = |sc: &Scanner<'_>| !sc.token().starts_with('}') && !sc.finished();
        sc.get_token(false);
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "SET_GEM" => out.set_gem = sc.get_number(false),
                "REMOVE_GEM" => out.remove_gem = sc.get_number(false),
                "GENERAL2UNIQUE" => out.general2unique = sc.get_number(false),
                "UNIQUE2ULTIMATE" => out.unique2ultimate = sc.get_number(false),
                "MAKE_GEM" => {
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while open(&sc) {
                        let ao = c_atoi(sc.token().as_bytes());
                        let row = MakeGemProb {
                            ability_option: ao,
                            general_prob: sc.get_number(false),
                            general_num: sc.get_number_i32(false),
                            unique_prob: sc.get_number(false),
                            unique_num: sc.get_number_i32(false),
                        };
                        // `map::insert`: the first row of a level wins
                        if !out.make_gem.iter().any(|m| m.ability_option == ao) {
                            out.make_gem.push(row);
                        }
                        sc.get_token(false);
                    }
                }
                "ULTIMATE_ENCHANT" => {
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while open(&sc) {
                        let level = c_atoi(sc.token().as_bytes());
                        let prob = sc.get_number_i32(false);
                        if !out.enchant.iter().any(|(l, _)| *l == level) {
                            out.enchant.push((level, prob));
                        }
                        sc.get_token(false);
                    }
                }
                _ => {}
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
    }
}
