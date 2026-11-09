//! Ultimate weapon rules (Plan/19 §12.7, ref/13 §18.8, ref/07c §8; src:_Common/UltimateWeapon.cpp): the
//! weapon classes, the result codes, the odds and the socket arithmetic the six handlers of
//! `flyff-game::item::ultimate` share with the browser.
//!
//! Probabilities are per 1 000 000 (`Ultimate_UltimateWeapon.txt`): an attempt succeeds iff
//! `xRandom(1_000_000) < p`; the safe enchant fails iff `xRandom(1_000_000) > p`.

use flyff_common::defs::itemkind::{IK1_WEAPON, WEAPON_GENERAL, WEAPON_ULTIMATE, WEAPON_UNIQUE};
use flyff_data::items_ext::ultimate::{MakeGemProb, UltimateTables};
use flyff_data::props::item::ItemProp;

/// `ULTIMATE_SUCCESS`.
pub const ULTIMATE_SUCCESS: i32 = 0;
/// `ULTIMATE_FAILED`.
pub const ULTIMATE_FAILED: i32 = 1;
/// `ULTIMATE_CANCEL`: refused, nothing consumed.
pub const ULTIMATE_CANCEL: i32 = 2;
/// `ULTIMATE_ISULTIMATE`.
pub const ULTIMATE_ISULTIMATE: i32 = 3;
/// `ULTIMATE_INVENTORY`: no room.
pub const ULTIMATE_INVENTORY: i32 = 4;
/// `ULTIMATE_ISNOTULTIMATE`.
pub const ULTIMATE_ISNOTULTIMATE: i32 = 5;

/// The denominator of every ultimate probability.
pub const ULTIMATE_PPM: u32 = 1_000_000;
/// The highest `+N` of an ultimate weapon (`ULTIMATE_ENCHANT` has rows 1..=10).
pub const MAX_ULTIMATE_AO: i32 = 10;
/// The first `+N` that opens a gem socket: sockets = `AO - 5`.
pub const GEM_SOCKET_FROM_AO: i32 = 5;
/// The `+N` a unique weapon needs before it can become an ultimate one.
pub const UNIQUE_TO_ULTIMATE_AO: i32 = 10;

/// The class of a weapon (`dwReferStat1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponClass {
    /// `WEAPON_GENERAL`.
    General,
    /// `WEAPON_UNIQUE`.
    Unique,
    /// `WEAPON_ULTIMATE`.
    Ultimate,
    /// Anything else (not a convertible weapon).
    Other,
}

/// The class of a prop: only `IK1_WEAPON` items have one.
pub fn weapon_class(p: &ItemProp) -> WeaponClass {
    if p.item_kind1 != IK1_WEAPON {
        return WeaponClass::Other;
    }
    refer_class(p.refer_stat1)
}

/// The class of a raw `dwReferStat1` (the ultimate checks of `SetGem`, `RemoveGem` and `EnchantWeapon` do not
/// look at `IK1_WEAPON`).
pub fn refer_class(refer_stat1: u32) -> WeaponClass {
    match refer_stat1 {
        WEAPON_GENERAL => WeaponClass::General,
        WEAPON_UNIQUE => WeaponClass::Unique,
        WEAPON_ULTIMATE => WeaponClass::Ultimate,
        _ => WeaponClass::Other,
    }
}

/// The number of ultimate gem sockets of a `+ao` ultimate weapon (`AO - 5`, none up to `+5`).
pub fn gem_sockets(ao: i32) -> usize {
    usize::try_from(ao - GEM_SOCKET_FROM_AO).unwrap_or(0)
}

/// `MakeGem`: what a destroyed `class` weapon of `+ao` yields: `(probability, number)`; `None` = no `MAKE_GEM`
/// row for that `+N` (the handler cancels) or a class that makes no gems.
pub fn make_gem_yield(t: &UltimateTables, class: WeaponClass, ao: i32) -> Option<(u32, i32)> {
    let row: &MakeGemProb = t.make_gem_prob(ao)?;
    match class {
        WeaponClass::General => Some((row.general_prob, row.general_num)),
        WeaponClass::Unique => Some((row.unique_prob, row.unique_num)),
        _ => None,
    }
}

/// `TransWeapon`: the conversion chance of `class` (general to unique `GENERAL2UNIQUE`, unique to ultimate
/// `UNIQUE2ULTIMATE`); a prop with `dwReferTarget2 != NULL_ID` overrides it (the value is the probability).
pub fn transmute_prob(t: &UltimateTables, class: WeaponClass, refer_target2: u32) -> Option<u32> {
    if refer_target2 != u32::MAX {
        return Some(refer_target2);
    }
    match class {
        WeaponClass::General => Some(t.general2unique),
        WeaponClass::Unique => Some(t.unique2ultimate),
        _ => None,
    }
}

/// The chance, per 1 000 000, to reach `+target` with an ultimate enchant (`ULTIMATE_ENCHANT`).
pub fn enchant_prob(t: &UltimateTables, target: i32) -> Option<u32> {
    t.enchant_prob(target).and_then(|p| u32::try_from(p).ok())
}

/// `true` when the roll `r` (`xRandom(1_000_000)`) succeeds against `p`.
pub fn roll_succeeds(r: u32, p: u32) -> bool {
    r < p
}

/// `true` when the roll fails the safe enchant (`(int)xRandom(1_000_000) > p`).
pub fn safe_roll_fails(r: u32, p: u32) -> bool {
    i64::from(r) > i64::from(p)
}
