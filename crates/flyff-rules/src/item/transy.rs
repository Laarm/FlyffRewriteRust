//! Item transy rules: the gender swap of an armour (Plan/19 §12.8, ref/13 §18.9, ref/07b §8.5;
//! src:WORLDSERVER/ItemUpgrade.cpp:1433-1542).
//!
//! The cash path takes one of two scrolls, split by the level limit of the armour: the `A` scroll for items
//! with `limit_level1 <= 60`, the `B` scroll for `>= 61` (both comparisons are unsigned, so an item without a
//! limit, `NULL_ID`, counts as high). The new item must be the opposite-gender twin: identical kinds, job,
//! hands, parts, item level, ability range and attack speed.

use flyff_common::defs::define::{SEX_FEMALE, SEX_MALE};
use flyff_common::defs::item::{II_CHR_SYS_SCR_ITEMTRANSY_A, II_CHR_SYS_SCR_ITEMTRANSY_B};
use flyff_common::defs::itemkind::{IK2_ARMOR, IK2_ARMORETC};
use flyff_data::props::item::ItemProp;

/// `true` when `scroll` is one of the two transy scrolls.
pub fn is_transy_scroll(scroll: u32) -> bool {
    scroll == II_CHR_SYS_SCR_ITEMTRANSY_A || scroll == II_CHR_SYS_SCR_ITEMTRANSY_B
}

/// The scroll check of `OnItemTransy`: `A` refuses a target above level 60, `B` one below 61; any other item
/// is no scroll. `limit_level1` is the prop's `dwLimitLevel1` (`NULL_ID` = no limit).
pub fn scroll_fits(scroll: u32, limit_level1: u32) -> bool {
    match scroll {
        II_CHR_SYS_SCR_ITEMTRANSY_A => limit_level1 <= 60,
        II_CHR_SYS_SCR_ITEMTRANSY_B => limit_level1 >= 61,
        _ => false,
    }
}

/// The scroll that fits an armour of `limit_level1`.
pub fn scroll_for(limit_level1: u32) -> u32 {
    if limit_level1 <= 60 {
        II_CHR_SYS_SCR_ITEMTRANSY_A
    } else {
        II_CHR_SYS_SCR_ITEMTRANSY_B
    }
}

fn is_armor_kind(p: &ItemProp) -> bool {
    p.item_kind2 == IK2_ARMOR || p.item_kind2 == IK2_ARMORETC
}

fn has_sex(p: &ItemProp) -> bool {
    p.item_sex == SEX_MALE || p.item_sex == SEX_FEMALE
}

/// An armour of a gender: the only kind of item a transy scroll can swap.
pub fn is_swappable(p: &ItemProp) -> bool {
    is_armor_kind(p) && has_sex(p)
}

/// `RunItemTransy`'s test: can `from` become `to`? Different items, both armour of a gender, different
/// genders, and the same kinds, job, hands, parts, item level, ability range and attack speed.
pub fn can_transy(from: &ItemProp, to: &ItemProp) -> bool {
    from.id != to.id
        && is_armor_kind(from)
        && has_sex(from)
        && is_armor_kind(to)
        && has_sex(to)
        && from.item_sex != to.item_sex
        && from.item_kind1 == to.item_kind1
        && from.item_kind2 == to.item_kind2
        && from.item_kind3 == to.item_kind3
        && from.item_job == to.item_job
        && from.handed == to.handed
        && from.parts == to.parts
        && from.item_lv == to.item_lv
        && from.ability_min == to.ability_min
        && from.ability_max == to.ability_max
        && from.attack_speed == to.attack_speed
}

/// The items `from` can become (for the transy window): every prop of `candidates` that passes
/// [`can_transy`], in the order given.
pub fn twins<'a>(
    from: &'a ItemProp,
    candidates: impl IntoIterator<Item = &'a ItemProp> + 'a,
) -> impl Iterator<Item = &'a ItemProp> + 'a {
    candidates.into_iter().filter(move |c| can_transy(from, c))
}
