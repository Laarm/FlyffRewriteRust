//! The odds and the roll rules of the item upgrades (Plan/19 §12.1-§12.4, §12.6.1; ref/13 §18; src:WORLDSERVER/
//! ItemUpgrade.cpp:119-1296, DPSrvr.cpp:6171-6295). Pure and shared: the server handlers roll with these
//! functions and the browser's upgrade windows show the same numbers.
//!
//! # Roll conventions
//!
//! `r = random_below(N)` is `0..N`. Most paths *fail when `r > p`* ([`fails_over`]): the real success chance
//! is `(p + 1) / N`, capped at 100 %. The legacy accessory and collector paths *succeed when `r < p`*
//! ([`succeeds_under`]): a chance of exactly `p / N`.
//!
//! # Tables
//!
//! [`UpgradeTables`] holds the six tables the paths read. The shipped values are the constants of this module
//! (`Resource/ItemUpgrade.lua`, `s.txt` `Accessory_Probability` / `Collecting_Enchant`); [`UpgradeTables::from_data`]
//! reads the loaded tables and falls back to the shipped ones for a table that is absent, so a server without
//! the script files (tests, tools) rolls with the repack's odds.

use super::upgrade::{accessory_prob, element_refine_prob, general_refine_prob};
use flyff_data::items_ext::accessory::AccessoryTable;
use flyff_data::items_ext::collecting::CollectingTable;
use flyff_data::lua_config::ItemUpgradeConfig;

/// Roll space of the general, element, accessory (safe) and piercing paths.
pub const SPACE_10000: u32 = 10_000;
/// Roll space of the collector path.
pub const SPACE_1000: u32 = 1_000;
/// Roll space of the stat scroll's tier roll.
pub const STAT_SCROLL_SPACE: u32 = 100;

/// `tGeneral` of `ItemUpgrade.lua`: the refine odds of +1..+10 (before the ENG penalty).
pub const SHIPPED_GENERAL: [i32; 10] =
    [10000, 10000, 7000, 6000, 5000, 4000, 3000, 2000, 1000, 500];
/// `tAttribute[n].nProb` of `ItemUpgrade.lua`: the element odds of level 1..20.
pub const SHIPPED_ATTRIBUTE: [i32; 20] = [
    10000, 10000, 9200, 8000, 6300, 5000, 4000, 3300, 2700, 2300, 2000, 1400, 1200, 900, 800, 750,
    700, 550, 520, 500,
];
/// `Accessory_Probability` of `s.txt`: the accessory odds by current `+N` (0..19).
pub const SHIPPED_ACCESSORY: [i32; 20] = [
    10000, 10000, 9200, 8000, 6300, 5000, 4000, 3300, 2700, 2300, 2000, 1400, 1200, 900, 800, 750,
    700, 550, 520, 500,
];
/// `Collecting_Enchant` of `s.txt`: the collector odds per 1000 by current `+N` (0..4).
pub const SHIPPED_COLLECTOR: [i32; 5] = [100, 100, 100, 100, 100];
/// `tSuitProb`: the piercing-size odds of a suit by the new size.
pub const SHIPPED_PIERCING_SUIT: [i32; 4] = [8000, 5000, 2000, 500];
/// `tWeaponProb`: the piercing-size odds of a weapon by the new size.
pub const SHIPPED_PIERCING_WEAPON: [i32; 10] =
    [10000, 10000, 7000, 6000, 5000, 4000, 3000, 2000, 1000, 500];

/// `MAX_AAO`: the highest accessory upgrade.
pub const MAX_ACCESSORY_AO: i32 = 20;
/// The refine level from which a failure destroys an unprotected item (`AO >= 3`, legacy paths).
pub const DESTROY_FROM_LEVEL: i32 = 3;
/// A smelting scroll adds this much to the odds (`+1000`) ...
pub const SMELTING_BONUS: i32 = 1_000;
/// ... a super smelting scroll this much (`+10000`, a sure thing).
pub const SUPER_SMELTING_BONUS: i32 = 10_000;
/// A general refine takes the smelting bonuses only below this `+N`.
pub const GENERAL_BONUS_BELOW: i32 = 7;
/// An element upgrade takes the `SMELTING2` bonus only below this level.
pub const ELEMENT_BONUS_BELOW: i32 = 10;
/// Penya for removing an element (`OnRemoveAttribute`).
pub const REMOVE_ELEMENT_COST: i64 = 100_000;
/// Penya for the awakening at the NPC (`OnAwakening`).
pub const AWAKENING_COST: i64 = 100_000;
/// `AddGold( -nCost )` of the piercing size (`OnPiercingSize`, `SmeltSafetyPiercingSize`).
pub const PIERCING_SIZE_COST: i64 = 100_000;

/// "Fails when `r > p`" (general, element, safe accessory and the safe paths).
pub fn fails_over(roll: u32, p: i32) -> bool {
    i64::from(roll) > i64::from(p)
}

/// "Succeeds when `r < p`" (legacy accessory and collector).
pub fn succeeds_under(roll: u32, p: i32) -> bool {
    i64::from(roll) < i64::from(p)
}

/// The odds the paths read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeTables {
    /// `tGeneral` (index = current `+N`).
    pub general: Vec<i32>,
    /// `(level, prob)` of the element upgrade, ascending by level (the odds to reach `level`).
    pub attribute: Vec<(i32, i32)>,
    /// The accessory odds (index = current `+N`).
    pub accessory: Vec<i32>,
    /// The collector odds per 1000 (index = current `+N`).
    pub collector: Vec<i32>,
    /// `tSuitProb` (index = the new size minus 1).
    pub piercing_suit: Vec<i32>,
    /// `tWeaponProb` (index = the new size minus 1).
    pub piercing_weapon: Vec<i32>,
}

impl Default for UpgradeTables {
    /// The shipped tables.
    fn default() -> Self {
        UpgradeTables {
            general: SHIPPED_GENERAL.to_vec(),
            attribute: SHIPPED_ATTRIBUTE
                .iter()
                .enumerate()
                .map(|(i, &p)| (i as i32 + 1, p))
                .collect(),
            accessory: SHIPPED_ACCESSORY.to_vec(),
            collector: SHIPPED_COLLECTOR.to_vec(),
            piercing_suit: SHIPPED_PIERCING_SUIT.to_vec(),
            piercing_weapon: SHIPPED_PIERCING_WEAPON.to_vec(),
        }
    }
}

impl UpgradeTables {
    /// The tables of the loaded data. A table that is absent (`None`, empty or - for the two probability
    /// lists of `s.txt` - all zero) is replaced by the shipped one.
    pub fn from_data(
        lua: Option<&ItemUpgradeConfig>,
        accessory: &AccessoryTable,
        collecting: &CollectingTable,
    ) -> UpgradeTables {
        let mut t = UpgradeTables::default();
        if let Some(cfg) = lua {
            if !cfg.general.is_empty() {
                t.general = cfg.general.clone();
            }
            if !cfg.attribute.is_empty() {
                let mut rows: Vec<(i32, i32)> =
                    cfg.attribute.iter().map(|a| (a.level, a.prob)).collect();
                rows.sort_by_key(|r| r.0);
                t.attribute = rows;
            }
            if !cfg.suit_prob.is_empty() {
                t.piercing_suit = cfg.suit_prob.clone();
            }
            if !cfg.weapon_prob.is_empty() {
                t.piercing_weapon = cfg.weapon_prob.clone();
            }
        }
        if accessory.probability.iter().any(|&p| p != 0) {
            t.accessory = accessory
                .probability
                .iter()
                .map(|&p| i32::try_from(p).unwrap_or(i32::MAX))
                .collect();
        }
        if !collecting.enchant.is_empty() {
            t.collector = collecting.enchant.clone();
        }
        t
    }

    /// `GetMaxGeneralEnchantSize`: the highest `+N` (10).
    pub fn max_general(&self) -> i32 {
        i32::try_from(self.general.len()).unwrap_or(i32::MAX)
    }

    /// `GetGeneralEnchantProb( ao )`: the odds, out of 10 000, to refine an item that has `+ao`, with the ENG
    /// penalty of KQ-12 (`0` when the table has no row, like the original).
    pub fn general_prob(&self, ao: i32) -> i32 {
        general_refine_prob(&self.general, ao).unwrap_or(0)
    }

    /// `GetMaxAttributeEnchantSize`: the number of element levels (20).
    pub fn max_attribute(&self) -> i32 {
        i32::try_from(self.attribute.len()).unwrap_or(i32::MAX)
    }

    /// `GetAttributeEnchantProb( level )`: the odds, out of 10 000, to raise an element of `level` (0 = a new
    /// element) by one; `0` without a row.
    pub fn attribute_prob(&self, level: i32) -> i32 {
        element_refine_prob(&self.attribute, level.saturating_add(1)).unwrap_or(0)
    }

    /// `GetProbability( ao )` of the accessory table (`0` beyond it).
    pub fn accessory_prob(&self, ao: i32) -> i32 {
        accessory_prob(&self.accessory, ao).unwrap_or(0)
    }

    /// `GetMaxCollectorLevel`: the number of collector upgrades (5).
    pub fn max_collector(&self) -> i32 {
        i32::try_from(self.collector.len()).unwrap_or(i32::MAX)
    }

    /// `GetEnchantProbability( ao )`: the collector odds, out of 1000 (`0` beyond the table).
    pub fn collector_prob(&self, ao: i32) -> i32 {
        accessory_prob(&self.collector, ao).unwrap_or(0)
    }

    /// `GetSizeProb`: the piercing-size odds for an item that goes from `size` to `size + 1` sockets
    /// (`suit` selects `tSuitProb`); `0` beyond the table.
    pub fn piercing_size_prob(&self, suit: bool, size: i32) -> i32 {
        let t = if suit {
            &self.piercing_suit
        } else {
            &self.piercing_weapon
        };
        accessory_prob(t, size).unwrap_or(0)
    }
}

/// Which path an odds query is about (the browser windows and the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    /// General refine +1..+10 (out of 10 000, fail when `r > p`).
    General,
    /// Element upgrade (out of 10 000, fail when `r > p`).
    Element,
    /// Accessory, legacy path (out of 10 000, succeed when `r < p`).
    Accessory,
    /// Accessory, safe path (out of 10 000, fail when `r > p`).
    AccessorySafe,
    /// Collector (out of 1000, succeed when `r < p`).
    Collector,
}

/// The odds and the cap of one path at the current level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Odds {
    /// The table value `p`.
    pub p: i32,
    /// The roll space `N`.
    pub space: u32,
    /// The success chance in parts per `space` after the roll rule (`(p + 1)` or `p`, capped at `space`).
    pub chance: u32,
    /// The highest level of the path; the item cannot be upgraded at or above it.
    pub max_level: i32,
}

impl UpgradeKind {
    /// The odds of raising an item that is at `level` by one; `None` when it is at the cap.
    pub fn odds(self, t: &UpgradeTables, level: i32) -> Option<Odds> {
        if level < 0 {
            return None;
        }
        let (p, space, max_level, strict) = match self {
            UpgradeKind::General => (t.general_prob(level), SPACE_10000, t.max_general(), false),
            UpgradeKind::Element => (
                t.attribute_prob(level),
                SPACE_10000,
                t.max_attribute(),
                false,
            ),
            UpgradeKind::Accessory => {
                (t.accessory_prob(level), SPACE_10000, MAX_ACCESSORY_AO, true)
            }
            UpgradeKind::AccessorySafe => (
                t.accessory_prob(level),
                SPACE_10000,
                MAX_ACCESSORY_AO,
                false,
            ),
            UpgradeKind::Collector => {
                (t.collector_prob(level), SPACE_1000, t.max_collector(), true)
            }
        };
        if level >= max_level {
            return None;
        }
        let chance = if strict {
            u32::try_from(p.max(0)).unwrap_or(0)
        } else {
            u32::try_from(p.saturating_add(1).max(0)).unwrap_or(0)
        }
        .min(space);
        Some(Odds {
            p,
            space,
            chance,
            max_level,
        })
    }
}

/// The smelting bonus a general refine takes at `+ao` from a smelting and a super smelting scroll (the
/// legacy buffs and the safe `SMELTING` scroll alike): `+1000` / `+10000` only below [`GENERAL_BONUS_BELOW`].
pub fn general_bonus(ao: i32, smelting: bool, super_smelting: bool) -> i32 {
    let mut bonus = 0;
    if ao < GENERAL_BONUS_BELOW {
        if smelting {
            bonus += SMELTING_BONUS;
        }
        if super_smelting {
            bonus += SUPER_SMELTING_BONUS;
        }
    }
    bonus
}

/// Whether a failure destroys the target of a legacy refine: from `+3` without protection.
pub fn legacy_failure_destroys(level: i32, protected: bool) -> bool {
    level >= DESTROY_FROM_LEVEL && !protected
}

// ---- the random stat scroll (DPSrvr.cpp:6171-6295) ------------------------------------------------------------

/// The value `+1..+4` of a stat scroll for `roll = random_below(100)`: with `improved`
/// (`features.improved_stat_scroll`) `<= 50` -> 1, `<= 70` -> 2, `<= 90` -> 3, else 4; vanilla `< 64`, `< 94`,
/// `< 99`, else.
pub fn stat_scroll_value(roll: u32, improved: bool) -> u8 {
    if improved {
        match roll {
            0..=50 => 1,
            51..=70 => 2,
            71..=90 => 3,
            _ => 4,
        }
    } else {
        match roll {
            0..=63 => 1,
            64..=93 => 2,
            94..=98 => 3,
            _ => 4,
        }
    }
}

/// The stat a random scroll adds to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollStat {
    /// STR.
    Str,
    /// DEX.
    Dex,
    /// INT.
    Int,
    /// STA.
    Sta,
}

/// The `RandomOptItem` ids (`propItemEtc.inc`) per scroll tier 1..4 (`nSTR`, `nDEX`, `nINT`, `nSTA`).
pub const SCROLL_IDS: [[u8; 4]; 4] = [
    [1, 9, 21, 37],
    [2, 10, 22, 38],
    [3, 11, 23, 39],
    [4, 12, 24, 40],
];

/// The stat of a random scroll item id, `None` for any other `IK3_RANDOM_SCROLL` (which writes 0).
pub fn scroll_stat(item_id: u32) -> Option<ScrollStat> {
    use flyff_common::defs::item::{
        II_SYS_SYS_SCR_RANDOMDEX, II_SYS_SYS_SCR_RANDOMINT, II_SYS_SYS_SCR_RANDOMSTA,
        II_SYS_SYS_SCR_RANDOMSTR,
    };
    match item_id {
        II_SYS_SYS_SCR_RANDOMSTR => Some(ScrollStat::Str),
        II_SYS_SYS_SCR_RANDOMDEX => Some(ScrollStat::Dex),
        II_SYS_SYS_SCR_RANDOMINT => Some(ScrollStat::Int),
        II_SYS_SYS_SCR_RANDOMSTA => Some(ScrollStat::Sta),
        _ => None,
    }
}

/// The stat-bonus id written into the low byte for `stat` at `value` (1..=4); 0 for an unknown scroll.
pub fn scroll_option_id(stat: Option<ScrollStat>, value: u8) -> u8 {
    let Some(stat) = stat else { return 0 };
    let row = match stat {
        ScrollStat::Str => 0,
        ScrollStat::Dex => 1,
        ScrollStat::Int => 2,
        ScrollStat::Sta => 3,
    };
    usize::from(value)
        .checked_sub(1)
        .and_then(|i| SCROLL_IDS.get(row).and_then(|r| r.get(i)))
        .copied()
        .unwrap_or(0)
}
