//! `collecting.inc` (+ the `Collecting_*` blocks of `s.txt`): the collector tables (`CCollectingProperty::
//! LoadScript`, Plan/14 §6.13, ref/13 §20; src:_Common/collecting.cpp:49-110).
//!
//! ```text
//! Cool '{' NUM* '}'                  seconds per collected item by collector level (AO 0..)
//! Enchant '{' NUM* '}'               success per 1000 of a collector upgrade by level (server override: s.txt)
//! nMaxBattery = NUM                  the battery (seconds of collecting)
//! Item '{' (ITEM PROB)* '}'          the loot table (PROB is added up; s.txt carries the real one)
//! ```
//!
//! This data set's `collecting.inc` has only `Cool` and `nMaxBattery`; the 5 `Enchant` entries and the 29 loot
//! items come from `Collecting_Enchant` / `Collecting_Item` in `s.txt` ([`super::server_script`], F10).

use serde::{Deserialize, Serialize};

/// One loot entry: the item and its **cumulative** probability out of 1,000,000,000 (`COLLECTING_ITEM`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectingItem {
    /// The item that is created.
    pub item: u32,
    /// The running sum of the probabilities up to and including this entry.
    pub cum_prob: u32,
}

/// The collector tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectingTable {
    /// `Cool`: seconds between two items by collector level (105, 90, 75, 60, 45, 30).
    pub cool: Vec<i32>,
    /// `Enchant`: upgrade success per 1000 by collector level (5 entries from `s.txt`, all 100).
    pub enchant: Vec<i32>,
    /// `nMaxBattery` (1800).
    pub max_battery: i32,
    /// The loot table in file order with cumulative probabilities.
    pub items: Vec<CollectingItem>,
}

/// The size of the loot probability space (`xRandom( 1000000000 )`).
pub const COLLECTING_SPACE: u32 = 1_000_000_000;

impl CollectingTable {
    /// `GetCool( ao )`. **KEEP-QUIRK:** the original returns `INT_MAX` for an option above the table (and reads
    /// one past the end for `ao == len`; here that is `INT_MAX` too).
    pub fn cool_for(&self, ability_option: i32) -> i32 {
        usize::try_from(ability_option)
            .ok()
            .and_then(|i| self.cool.get(i))
            .copied()
            .unwrap_or(i32::MAX)
    }

    /// `GetEnchantProbability( ao )`: per 1000, 0 above the table.
    pub fn enchant_probability(&self, ability_option: i32) -> i32 {
        usize::try_from(ability_option)
            .ok()
            .and_then(|i| self.enchant.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// `GetMaxCollectorLevel`: the number of `Enchant` entries.
    pub fn max_collector_level(&self) -> usize {
        self.enchant.len()
    }

    /// `GetItem` for a roll in `0..COLLECTING_SPACE`: the first entry with `roll < cum_prob`, `None` (the original
    /// returns item 0) when the roll is past the last cumulative value.
    pub fn pick(&self, roll: u32) -> Option<u32> {
        self.items
            .iter()
            .find(|e| roll < e.cum_prob)
            .map(|e| e.item)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_collecting, parse_loot_block};

#[cfg(feature = "load")]
mod parse {
    use super::{CollectingItem, CollectingTable};
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// Reads the `{ (ITEM PROB)* }` body whose `{` is the next token: the probabilities are summed up and every
    /// entry gets the running total (`Item` of `collecting.inc` and `Collecting_Item` of `s.txt`).
    pub(crate) fn parse_loot_block(sc: &mut Script<'_>) -> Vec<CollectingItem> {
        let mut out = Vec::new();
        let mut total = 0_u32;
        sc.get_token(); // {
        let mut item = sc.get_number();
        while open(sc) {
            total = total.wrapping_add(sc.get_number());
            out.push(CollectingItem {
                item,
                cum_prob: total,
            });
            item = sc.get_number();
        }
        out
    }

    /// `CCollectingProperty::LoadScript` over decoded `text`.
    pub(crate) fn parse_collecting(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> CollectingTable {
        let mut t = CollectingTable::default();
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
                "Cool" | "Enchant" => {
                    let is_cool = sc.token() == "Cool";
                    sc.get_token(); // {
                    let mut list = Vec::new();
                    let mut v = sc.get_number_i32();
                    while open(&sc) {
                        list.push(v);
                        v = sc.get_number_i32();
                    }
                    if is_cool {
                        t.cool.extend(list);
                    } else {
                        t.enchant.extend(list);
                    }
                }
                "nMaxBattery" => {
                    sc.get_token(); // =
                    t.max_battery = sc.get_number_i32();
                }
                "Item" => t.items.extend(parse_loot_block(&mut sc)),
                _ => {}
            }
            sc.get_token();
        }
        t
    }
}
