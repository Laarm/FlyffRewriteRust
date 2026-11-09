//! `PKSetting.inc` (`LoadScriptPK`, Plan/14 §6.11, ref/17 §8.5; src:_Common/Project.cpp:5113-5205).
//!
//! ```text
//! GENERAL_COLOR | READY_COLOR | CHAO_COLOR | GENERAL_ATTACK_TIME | READY_ATTACK_TIME | LIMITE_LEVEL |
//! DIE_PROPENSITY_MIN | DIE_PROPENSITY_MAX   NUM
//! PROPENSITY_PENALTY '{' { 'ADDPENALTY' step min max equipDropMin equipDropMax invenDropMin invenDropMax
//!                          shop bank fly guardAttack vendor }* '}'
//! LEVEL_EXP '{' { 'ADDEXP' level exp }* '}'
//! ```
//!
//! Reference values: colours `0xffffffff` / `0xffffc0cb` / `0xffff0000`, attack times 16 / 8, limit level
//! 20, die propensity 300..500, 8 penalty steps (the original keeps them in a `std::map` by step).
//! `GetLevelExp(lv)` defaults to 4,400,000 and `GetPropensityPenalty` falls back to the last step.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `GetLevelExp` default when the level has no `ADDEXP` row.
pub const DEFAULT_LEVEL_EXP: u32 = 4_400_000;

/// One `ADDPENALTY` step (`CHAO_PROPENSITY`). `min`/`max` are `DWORD` in C; they fit `i32` here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PropensityPenalty {
    /// The step (map key).
    pub step: i32,
    /// `dwPropensityMin`.
    pub min: i32,
    /// `dwPropensityMax`.
    pub max: i32,
    /// `nEquipDorpMin`.
    pub equip_drop_min: i32,
    /// `nEquipDorpMax`.
    pub equip_drop_max: i32,
    /// `nInvenDorpMin`.
    pub inven_drop_min: i32,
    /// `nInvenDorpMax`.
    pub inven_drop_max: i32,
    /// `nShop`.
    pub shop: i32,
    /// `nBank`.
    pub bank: i32,
    /// `nFly`.
    pub fly: i32,
    /// `nGuardAttack`.
    pub guard_attack: i32,
    /// `nVendor`.
    pub vendor: i32,
}

/// The PK (player-kill) settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkSetting {
    /// `GENERAL_COLOR` (ARGB).
    pub general_color: u32,
    /// `READY_COLOR` (ARGB).
    pub ready_color: u32,
    /// `CHAO_COLOR` (ARGB).
    pub chao_color: u32,
    /// `GENERAL_ATTACK_TIME`.
    pub general_attack_time: i32,
    /// `READY_ATTACK_TIME`.
    pub ready_attack_time: i32,
    /// `LIMITE_LEVEL`.
    pub limit_level: i32,
    /// `DIE_PROPENSITY_MIN`.
    pub die_propensity_min: i32,
    /// `DIE_PROPENSITY_MAX`.
    pub die_propensity_max: i32,
    /// The steps in ascending step order (`std::map` iteration order; the first row of a step wins).
    pub penalties: Vec<PropensityPenalty>,
    /// `LEVEL_EXP`: level to exp (`map::insert`: the first row of a level wins).
    pub level_exp: BTreeMap<i32, u32>,
}

impl PkSetting {
    /// `GetLevelExp(level)`: the `ADDEXP` value, else [`DEFAULT_LEVEL_EXP`].
    pub fn level_exp(&self, level: i32) -> u32 {
        self.level_exp
            .get(&level)
            .copied()
            .unwrap_or(DEFAULT_LEVEL_EXP)
    }

    /// `GetPropensityPenalty(step)`: the row of `step`, else the last (highest) step.
    pub fn penalty(&self, step: i32) -> Option<&PropensityPenalty> {
        self.penalties
            .iter()
            .find(|p| p.step == step)
            .or_else(|| self.penalties.last())
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_pk_setting;

#[cfg(feature = "load")]
mod parse {
    use super::{PkSetting, PropensityPenalty};
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// `CProject::LoadScriptPK` over decoded `text`.
    pub(crate) fn parse_pk_setting(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> PkSetting {
        let mut pk = PkSetting::default();
        let mut steps: BTreeMap<i32, PropensityPenalty> = BTreeMap::new();
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
                "GENERAL_COLOR" => pk.general_color = sc.get_number(),
                "READY_COLOR" => pk.ready_color = sc.get_number(),
                "CHAO_COLOR" => pk.chao_color = sc.get_number(),
                "GENERAL_ATTACK_TIME" => pk.general_attack_time = sc.get_number_i32(),
                "READY_ATTACK_TIME" => pk.ready_attack_time = sc.get_number_i32(),
                "LIMITE_LEVEL" => pk.limit_level = sc.get_number_i32(),
                "DIE_PROPENSITY_MIN" => pk.die_propensity_min = sc.get_number_i32(),
                "DIE_PROPENSITY_MAX" => pk.die_propensity_max = sc.get_number_i32(),
                "PROPENSITY_PENALTY" => {
                    sc.get_token(); // {
                    sc.get_token();
                    while !sc.finished() && !sc.token().starts_with('}') {
                        if sc.is("ADDPENALTY") {
                            let step = sc.get_number_i32();
                            let row = PropensityPenalty {
                                step,
                                min: sc.get_number_i32(),
                                max: sc.get_number_i32(),
                                equip_drop_min: sc.get_number_i32(),
                                equip_drop_max: sc.get_number_i32(),
                                inven_drop_min: sc.get_number_i32(),
                                inven_drop_max: sc.get_number_i32(),
                                shop: sc.get_number_i32(),
                                bank: sc.get_number_i32(),
                                fly: sc.get_number_i32(),
                                guard_attack: sc.get_number_i32(),
                                vendor: sc.get_number_i32(),
                            };
                            steps.entry(step).or_insert(row); // map::insert: first wins
                        }
                        sc.get_token();
                    }
                }
                "LEVEL_EXP" => {
                    sc.get_token(); // {
                    sc.get_token();
                    while !sc.finished() && !sc.token().starts_with('}') {
                        if sc.is("ADDEXP") {
                            let level = sc.get_number_i32();
                            let exp = sc.get_number();
                            pk.level_exp.entry(level).or_insert(exp);
                        }
                        sc.get_token();
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        pk.penalties = steps.into_values().collect();
        pk
    }
}
