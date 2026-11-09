//! `accessory.inc`: the upgrade bonuses of accessories (`CAccessoryProperty::LoadScript`, Plan/14 §6.13,
//! ref/17 §8.7; src:_Common/accessory.cpp:39-96).
//!
//! ```text
//! Accessory '{' (ITEM '{' (LEVEL '{' (DST ADJ)* '}')* '}')* '}'
//! Probability '{' NUM* '}'                       (server-side upgrade probabilities, overridden by s.txt)
//! ```
//!
//! The client shows the bonus of each accessory level, so the bonuses are kept per item and level
//! (`levels[level]`, `MAX_AAO + 1` = 21 levels). The first definition of an item wins (`map::insert`).
//! `probability` is the upgrade success table (per 10,000, one entry per current upgrade level, `MAX_AAO` = 20
//! entries, zero where nothing is defined); this data set's `accessory.inc` has none, `s.txt`
//! (`Accessory_Probability`, [`super::server_script`]) fills it.

use serde::{Deserialize, Serialize};

/// `MAX_AAO`: the highest accessory upgrade level.
pub const MAX_AAO: usize = 20;

/// One accessory: the `(DST, adjust)` pairs of each upgrade level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AccessoryDef {
    /// The accessory item.
    pub item: u32,
    /// `levels[level]` for level `0..=20`; always `MAX_AAO + 1` entries.
    pub levels: Vec<Vec<[i32; 2]>>,
}

/// The accessory table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessoryTable {
    /// The accessories in file order.
    pub items: Vec<AccessoryDef>,
    /// `m_adwProbability[MAX_AAO]`: the success chance (per 10,000) of upgrading from level `i` to `i + 1`;
    /// always `MAX_AAO` entries.
    pub probability: Vec<u32>,
}

impl Default for AccessoryTable {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            probability: vec![0; MAX_AAO],
        }
    }
}

impl AccessoryTable {
    /// `IsAccessory( item )`.
    pub fn is_accessory(&self, item: u32) -> bool {
        self.items.iter().any(|a| a.item == item)
    }

    /// `GetDst( item, ao )`: the `[dst, adjust]` pairs of the accessory at an upgrade level.
    pub fn bonus(&self, item: u32, ability_option: usize) -> Option<&[[i32; 2]]> {
        self.items
            .iter()
            .find(|a| a.item == item)
            .and_then(|a| a.levels.get(ability_option))
            .map(Vec::as_slice)
    }

    /// `GetProbability( ao )` (0 beyond the table).
    pub fn upgrade_probability(&self, ability_option: usize) -> u32 {
        self.probability.get(ability_option).copied().unwrap_or(0)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_accessory;

#[cfg(feature = "load")]
mod parse {
    use super::{AccessoryDef, AccessoryTable, MAX_AAO};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CAccessoryProperty::LoadScript` over decoded `text`.
    pub(crate) fn parse_accessory(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> AccessoryTable {
        let mut out = AccessoryTable::default();
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
                "Probability" => {
                    sc.get_token(); // {
                    let mut i = 0_usize;
                    let mut p = sc.get_number();
                    while open(&sc) {
                        match out.probability.get_mut(i) {
                            Some(slot) => *slot = p,
                            None => sc.report(
                                Severity::Error,
                                DiagCode::CapacityExceeded,
                                format!(
                                    "accessory.inc: Probability entry {i} > {MAX_AAO}; ignored"
                                ),
                            ),
                        }
                        i += 1;
                        p = sc.get_number();
                    }
                }
                "Accessory" => {
                    sc.get_token(); // {
                    let mut item = sc.get_number();
                    while open(&sc) {
                        sc.get_token(); // {
                        let mut levels = vec![Vec::new(); MAX_AAO + 1];
                        let mut level = sc.get_number();
                        while open(&sc) {
                            sc.get_token(); // {
                            let mut dst = sc.get_number_i32();
                            while open(&sc) {
                                let adj = sc.get_number_i32();
                                match levels.get_mut(level as usize) {
                                    Some(l) => l.push([dst, adj]),
                                    None => sc.report(
                                        Severity::Error,
                                        DiagCode::IndexOutOfRange,
                                        format!(
                                            "accessory.inc: level {level} > {MAX_AAO}; ignored"
                                        ),
                                    ),
                                }
                                dst = sc.get_number_i32();
                            }
                            level = sc.get_number();
                        }
                        if out.items.iter().any(|a| a.item == item) {
                            sc.report(
                                Severity::Warning,
                                DiagCode::PropIdDuplicate,
                                format!(
                                    "accessory.inc: item {item} is defined twice; the first wins"
                                ),
                            );
                        } else {
                            out.items.push(AccessoryDef { item, levels });
                        }
                        item = sc.get_number();
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        out
    }
}
