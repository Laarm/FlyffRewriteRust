//! `s.txt`: the server-only probabilities that override or extend the accessory, collecting and pet tables
//! (`CProject::LoadServerScript`, `__JEFF_11_3` "server/client resource split"; Plan/14 §5.4 F10, §6.13, ref/17
//! §8.5; src:_Common/Project.cpp:5397-5507).
//!
//! ```text
//! Accessory_Probability '{' NUM* '}'          -> AccessoryTable::probability[i]     (positional overwrite)
//! Collecting_Enchant '{' NUM* '}'             -> CollectingTable::enchant           (per 1000)
//! Collecting_Item '{' (ITEM PROB)* '}'        -> CollectingTable::items             (running sum)
//! Pet_LevelupAvail '{' (PROB{9})* '}'         -> PetTables::levelup_avail           (one row per level from D)
//! Pet_AddLifeProbability '{' NUM* '}'         -> PetTables::add_life_probability
//! ```
//!
//! It is loaded after `pet.inc`, `collecting.inc` and `accessory.inc` (L57 after L48-L50), so it overrides what
//! those files defined. **FIX (no data impact):** the original `push_back`s the three list-typed blocks
//! (`Collecting_Enchant`, `Collecting_Item`, `Pet_AddLifeProbability`) onto whatever the earlier file loaded, which
//! can leave a stale prefix; the plan's wording is "replaces", and here a block that `s.txt` contains **replaces**
//! the list (when the earlier file had none, which is the case for the shipped data, both readings agree). A
//! block that is absent leaves the earlier tables alone. `Accessory_Probability` and `Pet_LevelupAvail` overwrite
//! in place like the original (`ptr[i++]`, rows from `PL_D`).

use super::accessory::AccessoryTable;
use super::collecting::CollectingTable;
use super::pet::PetTables;

/// What `s.txt` changed (for the load counts and the tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerScriptStats {
    /// `Accessory_Probability` entries written.
    pub accessory_probability: u32,
    /// `Collecting_Enchant` entries.
    pub collecting_enchant: u32,
    /// `Collecting_Item` entries.
    pub collecting_items: u32,
    /// `Pet_LevelupAvail` rows.
    pub pet_levelup_rows: u32,
    /// `Pet_AddLifeProbability` entries.
    pub pet_add_life: u32,
}

/// The pet tables, the accessory table and the collecting table that `s.txt` patches.
pub struct ServerScriptTargets<'a> {
    /// `Accessory_Probability`.
    pub accessory: &'a mut AccessoryTable,
    /// `Collecting_Enchant`, `Collecting_Item`.
    pub collecting: &'a mut CollectingTable,
    /// `Pet_LevelupAvail`, `Pet_AddLifeProbability`.
    pub pet: &'a mut PetTables,
}

#[cfg(feature = "load")]
pub(crate) use parse::apply_server_script;

#[cfg(feature = "load")]
mod parse {
    use super::{ServerScriptStats, ServerScriptTargets};
    use crate::items_ext::collecting::parse_loot_block;
    use crate::items_ext::pet::MAX_PET_AVAIL_LEVEL;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `LoadServerScript` over decoded `text`: patches the three tables in place.
    pub(crate) fn apply_server_script(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        t: ServerScriptTargets<'_>,
    ) -> ServerScriptStats {
        let mut stats = ServerScriptStats::default();
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
                "Accessory_Probability" => {
                    sc.get_token(); // {
                    let mut i = 0_usize;
                    let mut p = sc.get_number();
                    while open(&sc) {
                        match t.accessory.probability.get_mut(i) {
                            Some(slot) => {
                                *slot = p;
                                stats.accessory_probability += 1;
                            }
                            None => sc.report(
                                Severity::Error,
                                DiagCode::CapacityExceeded,
                                format!("s.txt: Accessory_Probability entry {i} is beyond MAX_AAO; ignored"),
                            ),
                        }
                        i += 1;
                        p = sc.get_number();
                    }
                }
                "Collecting_Enchant" => {
                    sc.get_token(); // {
                    let mut list = Vec::new();
                    let mut p = sc.get_number_i32();
                    while open(&sc) {
                        list.push(p);
                        p = sc.get_number_i32();
                    }
                    stats.collecting_enchant = u32::try_from(list.len()).unwrap_or(u32::MAX);
                    t.collecting.enchant = list;
                }
                "Collecting_Item" => {
                    let items = parse_loot_block(&mut sc);
                    stats.collecting_items = u32::try_from(items.len()).unwrap_or(u32::MAX);
                    t.collecting.items = items;
                }
                "Pet_LevelupAvail" => {
                    // rows start at PL_D; the egg row does not exist here
                    sc.get_token(); // {
                    let mut rows = Vec::new();
                    let mut first = sc.get_number();
                    while open(&sc) {
                        let mut row = vec![first];
                        for _ in 1..MAX_PET_AVAIL_LEVEL {
                            row.push(sc.get_number());
                        }
                        rows.push(row);
                        first = sc.get_number();
                    }
                    stats.pet_levelup_rows = u32::try_from(rows.len()).unwrap_or(u32::MAX);
                    t.pet.levelup_avail = rows;
                }
                "Pet_AddLifeProbability" => {
                    sc.get_token(); // {
                    let mut list = Vec::new();
                    let mut p = sc.get_number();
                    while open(&sc) {
                        list.push(p);
                        p = sc.get_number();
                    }
                    stats.pet_add_life = u32::try_from(list.len()).unwrap_or(u32::MAX);
                    t.pet.add_life_probability = list;
                }
                _ => {}
            }
            sc.get_token();
        }
        stats
    }
}
