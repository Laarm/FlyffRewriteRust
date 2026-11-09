//! `MoverEx`: the per-mover AI, drop and behaviour block of `propMoverEx.inc` (Plan/14 §6.5.2, ref/17
//! §8.9, ref/12 §9.2; src:_Common/Project.cpp:2646-2976, ProjectLux.cpp:25-466).
//!
//! The types, and with feature `load` the whole loader of L32 (`parse::parse_mover_ex`): every statement of a
//! block (`m_nAttackFirstRange`, `SetEvasion`, `SetRunAway`, `SetCallHelper`, the `m_nAttackItem*` family,
//! `m_nMagicReflection`, `m_nImmortality`, `m_bBlow`, `m_nChangeTargetRand`, `m_dwAttackMoveDelay`,
//! `m_dwRunawayDelay`, `Maxitem`, `DropItem`, `DropGold`, `DropKind`, `Transform`, `randomItem {}`) and the
//! `AI {}` block (the private `ai` module), plus `parse::apply_drop_events`, the `propDropEvent.inc` merge of F9 (L39).
//! `MoverEx::present` is set for every mover that has a block in the file.

#[cfg(feature = "load")]
mod ai;

use crate::types::{ItemId, MoverId};
use serde::{Deserialize, Serialize};

/// `m_nHelper` entry of `SetCallHelper(hp, mover, num, party)` (at most 5 per mover).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallHelper {
    /// The helper mover id.
    pub idx: i16,
    /// How many helpers.
    pub num: i16,
    /// Whether they join the party.
    pub party: i16,
}

/// The `AI { #SCAN{} #BATTLE{} #MOVE{} }` block (fields per ref/12 §9.2; semantics owned by doc 18).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoverAi {
    /// `#SCAN job`.
    pub scan_job: i32,
    /// `#SCAN quest`.
    pub scan_quest_id: u32,
    /// `#SCAN item`.
    pub scan_item_idx: u32,
    /// `#SCAN chao`.
    pub scan_chao: i32,
    /// Melee attack flag.
    pub melee_attack: u8,
    /// HP condition.
    pub hp_cond: i32,
    /// Level condition.
    pub lv_cond: i32,
    /// Recovery condition.
    pub recv_cond: u8,
    /// Recovery condition: self.
    pub recv_cond_me: i32,
    /// Recovery condition: how.
    pub recv_cond_how: i32,
    /// Recovery condition: MP.
    pub recv_cond_mp: i32,
    /// Recovery condition: who.
    pub recv_cond_who: u8,
    /// Range attack per job (bit 0x80 = keep distance).
    pub range_attack: [u8; 32],
    /// Summon probability.
    pub summ_prob: i32,
    /// Summon count.
    pub summ_num: i32,
    /// Summoned mover id.
    pub summ_id: i32,
    /// Helper selection.
    pub help_who: u8,
    /// Helper range multiplier.
    pub help_range_mul: i32,
    /// Helper unit interval in ms.
    pub unit_help_ms: u32,
    /// Berserk HP threshold.
    pub berserk_hp: i32,
    /// Berserk damage multiplier.
    pub berserk_dmg_mul: f32,
    /// Loot flag.
    pub loot: i32,
    /// Loot probability.
    pub loot_prob: i32,
}

impl Default for MoverAi {
    fn default() -> Self {
        Self {
            scan_job: 0,
            scan_quest_id: 0,
            scan_item_idx: 0,
            scan_chao: 0,
            melee_attack: 0,
            hp_cond: 0,
            lv_cond: 0,
            recv_cond: 0,
            recv_cond_me: 0,
            recv_cond_how: 0,
            recv_cond_mp: 0,
            recv_cond_who: 0,
            range_attack: [0; 32],
            summ_prob: 0,
            summ_num: 0,
            summ_id: 0,
            help_who: 0,
            help_range_mul: 0,
            unit_help_ms: 0,
            berserk_hp: 0,
            berserk_dmg_mul: 0.0,
            loot: 0,
            loot_prob: 0,
        }
    }
}

/// `DROPTYPE_NORMAL` / `DROPTYPE_SEED`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DropType {
    /// A normal item drop.
    #[default]
    Normal = 0,
    /// A gold drop (`DropGold`: probability `0xFFFFFFFF`, `number..=number2`).
    Seed = 1,
}

/// One drop entry. The `probability` scale is "hit when `xRandom(3_000_000_000) <= probability`".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropItem {
    /// Normal or gold.
    pub dtype: DropType,
    /// The item (`DropItem(item, ..)`; 0 is an Error).
    pub item: ItemId,
    /// The probability.
    pub probability: u32,
    /// The item level.
    pub level: u32,
    /// The amount (gold: the minimum).
    pub number: u32,
    /// Gold: the maximum.
    pub number2: u32,
}

/// `DropKind(ik3, a, b)`: the unique range derived from the mover level (F8).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropKind {
    /// The item kind.
    pub ik3: u32,
    /// `max(level - 5, 1)`.
    pub min_uniq: i16,
    /// `max(level - 2, 1)`.
    pub max_uniq: i16,
}

/// The drop generator of a mover.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropTable {
    /// `Maxitem` (`m_dwMax`).
    pub max_item: u32,
    /// File order, then `propDropEvent` appends (F9).
    pub items: Vec<DropItem>,
    /// The drop kinds.
    pub kinds: Vec<DropKind>,
}

/// `Transform(hpRate, mover)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonsterTransform {
    /// HP rate at which the mover transforms.
    pub hp_rate: f32,
    /// The target mover.
    pub mover: MoverId,
}

/// The `propMoverEx.inc` block of a mover. `Default` holds the values the original sets before it reads a
/// block (`runaway_hp = -1`, `call_hp = -1`, `change_target_rand = 10`, `runaway_delay_ms = 1000`,
/// `attack_first_range = 10`) with `present == false`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoverEx {
    /// A block existed for the mover.
    pub present: bool,
    /// `SetEvasion` HP.
    pub evasion_hp: i16,
    /// `SetEvasion` seconds.
    pub evasion_sec: i16,
    /// `SetRunAway` HP (default -1).
    pub runaway_hp: i16,
    /// `SetCallHelper` HP (default -1).
    pub call_hp: i16,
    /// `SetCallHelper` entries (at most 5).
    pub call_helpers: Vec<CallHelper>,
    /// `m_nChangeTargetRand` (default 10).
    pub change_target_rand: i32,
    /// `m_dwAttackMoveDelay`.
    pub attack_move_delay: i16,
    /// `m_dwRunawayDelay` in ms (default 1000).
    pub runaway_delay_ms: i16,
    /// Party attack.
    pub party_attack: i16,
    /// `m_nAttackFirstRange` (default 10, must be 1..=10).
    pub attack_first_range: i32,
    /// `m_nAttackItemNear`.
    pub attack_item_near: i16,
    /// `m_nAttackItemFar`.
    pub attack_item_far: i16,
    /// `m_nAttackItem1..4`.
    pub attack_item: [i16; 4],
    /// `m_nAttackItemSec`.
    pub attack_item_sec: i16,
    /// `m_nMagicReflection`.
    pub magic_reflection: i32,
    /// `m_nImmortality`.
    pub immortality: i32,
    /// `m_bBlow`.
    pub blow: i32,
    /// The AI block.
    pub ai: MoverAi,
    /// The drops (item rows from the file and from `propDropEvent`).
    pub drops: DropTable,
    /// `Transform(..)`.
    pub transform: Option<MonsterTransform>,
}

impl Default for MoverEx {
    fn default() -> Self {
        Self {
            present: false,
            evasion_hp: 0,
            evasion_sec: 0,
            runaway_hp: -1,
            call_hp: -1,
            call_helpers: Vec::new(),
            change_target_rand: 10,
            attack_move_delay: 0,
            runaway_delay_ms: 1000,
            party_attack: 0,
            attack_first_range: 10,
            attack_item_near: 0,
            attack_item_far: 0,
            attack_item: [0; 4],
            attack_item_sec: 0,
            magic_reflection: 0,
            immortality: 0,
            blow: 0,
            ai: MoverAi::default(),
            drops: DropTable::default(),
            transform: None,
        }
    }
}

#[cfg(feature = "load")]
pub use parse::DropLoadStats;
#[cfg(feature = "load")]
pub(crate) use parse::{apply_drop_events, parse_mover_ex};

/// The loaders of `propMoverEx.inc` and `propDropEvent.inc` (`LoadPropMoverEx`, `LoadDropEvent`).
#[cfg(feature = "load")]
mod parse {
    use super::{CallHelper, DropItem, DropKind, DropType, MonsterTransform, ai};
    use crate::items_ext::except::ItemExcept;
    use crate::props::mover::MoverProp;
    use crate::types::IdTable;
    use flyff_common::defs::item::II_GEN_SKILL_BUFFBREAKER;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// What a load of `propMoverEx.inc` / `propDropEvent.inc` did.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct DropLoadStats {
        /// Mover blocks read (`propMoverEx.inc`).
        pub blocks: u32,
        /// Blocks of a mover that does not exist (skipped).
        pub unknown_movers: u32,
        /// `DropItem` rows added from the blocks, or appended by the drop events.
        pub items: u32,
        /// `DropGold` rows added.
        pub gold: u32,
        /// `DropKind` rows added.
        pub kinds: u32,
        /// `DropItem` rows of the drop events that were skipped (items in the world-drop exclusion set).
        pub excluded: u32,
        /// `AI { }` blocks read.
        pub ai_blocks: u32,
        /// `Transform( .. )` statements read.
        pub transforms: u32,
        /// `DropItem` lines read from `propDropEvent.inc` (C16: 200).
        pub event_rows: u32,
    }

    /// Skips the body of a `randomItem { ... }` block whose `randomItem` was just read: `InterpretRandomItem`
    /// reads the `{` and then tokens up to the **first** `}` (no nesting).
    fn skip_random_item(s: &mut Script<'_>) {
        s.get_token(); // {
        s.get_token();
        while !s.finished() && !s.token().starts_with('}') {
            s.get_token();
        }
    }

    /// `CProject::LoadPropMoverEx` (Plan/14 §6.5.2, F8): per mover block `MI '{' stmt* '}'`. Statement names are
    /// case-sensitive, `AI` is not; a `;` is skipped; any other token is skipped on its own, as the original does.
    ///
    /// * Every block starts from the values the original sets before it reads a block (`runaway_hp = -1`,
    ///   `call_hp = -1`, `change_target_rand = 10`, `runaway_delay_ms = 1000`, `attack_first_range = 10`, no
    ///   helpers, no evasion, `attack_move_delay = 0`) and marks the mover's `ex.present`; the drop table keeps
    ///   what earlier blocks added.
    /// * `DropKind(ik3, a, b)`: `a` and `b` are read and ignored; the range derives from the mover level (F8).
    /// * `m_nAttackFirstRange` outside 1..=10 is an Error (the value is stored). A 6th `SetCallHelper` is an Error
    ///   and ignored. A `DropItem` of item 0 (an unresolved define) is an Error and not added.
    /// * A block of an unknown mover id is parsed and dropped (Warning); the original loops forever on it.
    pub(crate) fn parse_mover_ex(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        movers: &mut IdTable<MoverProp>,
        diags: &mut Diagnostics,
    ) -> DropLoadStats {
        let mut stats = DropLoadStats::default();
        let mut s = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut id = s.get_number();
        while !s.finished() {
            stats.blocks += 1;
            let level = movers.get(id).map(|p| p.level);
            let mut ex = movers.get(id).map(|p| p.ex.clone());
            if ex.is_none() {
                stats.unknown_movers += 1;
                s.report(
                    Severity::Warning,
                    DiagCode::RefUnresolved,
                    format!("propMoverEx.inc: a block for the unknown mover {id}"),
                );
            }
            let mut block = ex.take().unwrap_or_default();
            // the initialisation before a block (`LoadPropMoverEx`)
            block.present = true;
            block.evasion_hp = 0;
            block.evasion_sec = 0;
            block.runaway_hp = -1;
            block.call_hp = -1;
            block.call_helpers.clear();
            block.change_target_rand = 10;
            block.attack_move_delay = 0;
            block.party_attack = 0;
            block.runaway_delay_ms = 1000;
            block.attack_first_range = 10;
            let mut max_item: Option<u32> = None;
            let mut rows: Vec<DropItem> = Vec::new();
            let mut kinds: Vec<DropKind> = Vec::new();
            s.get_token(); // {
            s.get_token();
            while !s.finished() && !s.token().starts_with('}') {
                if s.token() == ";" {
                    s.get_token();
                    continue;
                }
                let word = s.token().to_owned();
                if word.eq_ignore_ascii_case("AI") {
                    stats.ai_blocks += 1;
                    ai::parse_ai(&mut s, id, &mut block, movers);
                } else {
                    match word.as_str() {
                        "m_nAttackFirstRange" => {
                            s.get_token(); // =
                            block.attack_first_range = s.get_number_i32();
                            if !(1..=10).contains(&block.attack_first_range) {
                                s.report(
                                    Severity::Error,
                                    DiagCode::ValueOutOfRange,
                                    format!(
                                        "propMoverEx.inc: mover {id}: m_nAttackFirstRange {} is outside 1..=10",
                                        block.attack_first_range
                                    ),
                                );
                            }
                        }
                        "SetEvasion" => {
                            s.get_token(); // (
                            block.evasion_hp = s.get_number_i32() as i16;
                            s.get_token(); // ,
                            block.evasion_sec = s.get_number_i32() as i16;
                            s.get_token(); // )
                        }
                        "SetRunAway" => {
                            s.get_token(); // (
                            block.runaway_hp = s.get_number_i32() as i16;
                            s.get_token(); // , or )
                            if s.token() == "," {
                                s.get_number();
                                s.get_token(); // ,
                                s.get_number();
                                s.get_token(); // )
                            }
                        }
                        "SetCallHelper" => {
                            s.get_token(); // (
                            block.call_hp = s.get_number_i32() as i16;
                            s.get_token(); // ,
                            let idx = s.get_number_i32() as i16;
                            s.get_token(); // ,
                            let num = s.get_number_i32() as i16;
                            s.get_token(); // ,
                            let party = s.get_number_i32() as i16;
                            s.get_token(); // )
                            if block.call_helpers.len() >= 5 {
                                s.report(
                                    Severity::Error,
                                    DiagCode::CapacityExceeded,
                                    format!("propMoverEx.inc: mover {id}: more than 5 SetCallHelper entries; the extra one is ignored"),
                                );
                            } else {
                                block.call_helpers.push(CallHelper { idx, num, party });
                            }
                        }
                        "m_nAttackItemNear" => {
                            s.get_token();
                            block.attack_item_near = s.get_number_i32() as i16;
                        }
                        "m_nAttackItemFar" => {
                            s.get_token();
                            block.attack_item_far = s.get_number_i32() as i16;
                        }
                        "m_nAttackItem1" | "m_nAttackItem2" | "m_nAttackItem3"
                        | "m_nAttackItem4" => {
                            let slot =
                                usize::from(word.as_bytes().last().copied().unwrap_or(b'1') - b'1');
                            s.get_token();
                            let v = s.get_number_i32() as i16;
                            if let Some(a) = block.attack_item.get_mut(slot) {
                                *a = v;
                            }
                        }
                        "m_nAttackItemSec" => {
                            s.get_token();
                            block.attack_item_sec = s.get_number_i32() as i16;
                        }
                        "m_nMagicReflection" => {
                            s.get_token();
                            block.magic_reflection = i32::from(s.get_number_i32() as i16);
                        }
                        "m_nImmortality" => {
                            s.get_token();
                            block.immortality = i32::from(s.get_number_i32() as i16);
                        }
                        "m_bBlow" => {
                            s.get_token();
                            block.blow = s.get_number_i32();
                        }
                        "m_nChangeTargetRand" => {
                            s.get_token();
                            block.change_target_rand = i32::from(s.get_number_i32() as i16);
                        }
                        "m_dwAttackMoveDelay" => {
                            s.get_token();
                            block.attack_move_delay = s.get_number() as i16;
                        }
                        "m_dwRunawayDelay" => {
                            s.get_token();
                            block.runaway_delay_ms = s.get_number() as i16;
                        }
                        "randomItem" => skip_random_item(&mut s),
                        "Maxitem" => {
                            s.get_token(); // =
                            max_item = Some(s.get_number());
                        }
                        "DropItem" => {
                            s.get_token(); // (
                            let item = s.get_number();
                            s.get_token(); // ,
                            let probability = s.get_number();
                            s.get_token(); // ,
                            let level = s.get_number();
                            s.get_token(); // ,
                            let number = s.get_number();
                            s.get_token(); // )
                            if item == 0 {
                                s.report(
                                    Severity::Error,
                                    DiagCode::PropIdInvalid,
                                    format!("propMoverEx.inc: mover {id}: DropItem of item 0 (an unresolved define?)"),
                                );
                            } else {
                                rows.push(DropItem {
                                    dtype: DropType::Normal,
                                    item,
                                    probability,
                                    level,
                                    number,
                                    number2: 0,
                                });
                            }
                        }
                        "DropKind" => {
                            s.get_token(); // (
                            let ik3 = s.get_number();
                            s.get_token(); // ,
                            let _a = s.get_number_i32();
                            s.get_token(); // ,
                            let _b = s.get_number_i32();
                            s.get_token(); // )
                            let lv = i64::from(level.unwrap_or(0));
                            kinds.push(DropKind {
                                ik3,
                                min_uniq: (lv - 5).max(1) as i16,
                                max_uniq: (lv - 2).max(1) as i16,
                            });
                        }
                        "DropGold" => {
                            s.get_token(); // (
                            let number = s.get_number();
                            s.get_token(); // ,
                            let number2 = s.get_number();
                            s.get_token(); // )
                            rows.push(DropItem {
                                dtype: DropType::Seed,
                                item: 0,
                                probability: 0xFFFF_FFFF,
                                level: 0,
                                number,
                                number2,
                            });
                        }
                        "Transform" => {
                            s.get_token(); // (
                            let hp_rate = s.get_float();
                            s.get_token(); // ,
                            let mover = s.get_number();
                            s.get_token(); // )
                            stats.transforms += 1;
                            block.transform = Some(MonsterTransform { hp_rate, mover });
                        }
                        _ => {}
                    }
                }
                s.get_token();
            }
            if let Some(p) = movers.get_mut(id) {
                if let Some(m) = max_item {
                    block.drops.max_item = m;
                }
                stats.gold += rows.iter().filter(|r| r.dtype == DropType::Seed).count() as u32;
                stats.items += rows.iter().filter(|r| r.dtype == DropType::Normal).count() as u32;
                stats.kinds += kinds.len() as u32;
                block.drops.items.extend(rows);
                block.drops.kinds.extend(kinds);
                p.ex = block;
            }
            id = s.get_number();
        }
        stats
    }

    /// `CProject::LoadDropEvent` (F9): every `DropItem(item, prob, level, number, min_level, max_level)` is
    /// appended to the drop table of **every** mover with `id != 0` and `min_level <= level <= max_level`, in
    /// ascending mover id and file order. Items of the world-drop exclusion set (`except.txt`, `worldDrop = 0`)
    /// are skipped; `II_GEN_SKILL_BUFFBREAKER` loses half of its probability (non-KOR, truncated).
    pub(crate) fn apply_drop_events(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        except: &ItemExcept,
        movers: &mut IdTable<MoverProp>,
        diags: &mut Diagnostics,
    ) -> DropLoadStats {
        let mut stats = DropLoadStats::default();
        let mut s = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        loop {
            s.get_token();
            if s.finished() {
                break;
            }
            if s.token() != "DropItem" {
                continue;
            }
            s.get_token(); // (
            let item = s.get_number();
            s.get_token(); // ,
            let mut probability = s.get_number();
            s.get_token(); // ,
            let level = s.get_number();
            s.get_token(); // ,
            let number = s.get_number();
            s.get_token(); // ,
            let min_level = s.get_number();
            s.get_token(); // ,
            let max_level = s.get_number();
            s.get_token(); // )
            stats.event_rows += 1;
            if except.world_drop_excluded.contains(&item) {
                stats.excluded += 1;
                continue;
            }
            if item == II_GEN_SKILL_BUFFBREAKER {
                probability = (f64::from(probability) * 0.5) as u32;
            }
            let ids: Vec<u32> = movers.ids().to_vec();
            for id in ids {
                let Some(p) = movers.get_mut(id) else {
                    continue;
                };
                if id != 0 && p.level >= min_level && p.level <= max_level {
                    p.ex.drops.items.push(DropItem {
                        dtype: DropType::Normal,
                        item,
                        probability,
                        level,
                        number,
                        number2: 0,
                    });
                    stats.items += 1;
                }
            }
        }
        stats
    }
}
