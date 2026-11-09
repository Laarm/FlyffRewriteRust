//! Piercing rules (Plan/19 §12.5, ref/13 §18.6, ref/07b §8.2-8.4; src:_Common/Item.cpp:739-787,
//! WORLDSERVER/ItemUpgrade.cpp:119-380): which items take sockets, the socket limits, the costs and the
//! size odds.
//!
//! **`IsPierceAble` and `__NO_PIERCING`.** The original function is an `if / else if` chain; with
//! `__NO_PIERCING` (`features.no_piercing`, the repack default) the suit/shield/weapon branches are compiled out
//! and the vis-pet `else if` binds to `if (bSize)`: a vis pet is pierceable only when the size is *not* asked
//! for (PL-070). With the switch off the chain is complete: a suit takes `IK3_SOCKETCARD` up to 4 sockets, a
//! shield, a direct weapon or a magic weapon `IK3_SOCKETCARD2` up to 10, a vis pet `IK3_VIS` up to 9.

use flyff_common::defs::itemkind::{
    IK2_WEAPON_DIRECT, IK2_WEAPON_MAGIC, IK3_SHIELD, IK3_SOCKETCARD, IK3_SOCKETCARD2, IK3_SUIT,
    IK3_VIS,
};
use flyff_data::items_ext::item_etc::PiercingDef;
use flyff_data::props::item::ItemProp;

/// `MAX_PIERCING_SUIT`.
pub const MAX_PIERCING_SUIT: usize = 4;
/// `MAX_PIERCING_WEAPON` (weapons and shields).
pub const MAX_PIERCING_WEAPON: usize = 10;
/// `MAX_VIS`.
pub const MAX_VIS: usize = 9;
/// The price of one more socket (`PiercingSize`).
pub const PIERCING_SIZE_COST: i32 = 100_000;
/// The price of taking the last card out (`PiercingRemove`).
pub const PIERCING_REMOVE_COST: i32 = 1_000_000;
/// The denominator of the size odds (`tSuitProb` / `tWeaponProb`).
pub const PIERCING_SIZE_DENOM: u32 = 10_000;

/// `CItemElem::IsPierceAble(card_kind3, size)`: can the item take (`size == false`) a card of `card_kind3`
/// (`None` = any, the legacy `NULL_ID`) or (`size == true`) one more socket? `pierced_size` is the current
/// socket count and `is_vis` the `IsVisPet()` answer (the prop kind or a converted egg).
pub fn is_pierce_able(
    prop: &ItemProp,
    pierced_size: usize,
    is_vis: bool,
    card_kind3: Option<u32>,
    size: bool,
    no_piercing: bool,
) -> bool {
    let n = pierced_size + usize::from(size);
    let fits = |max: usize, card: u32| n <= max && card_kind3.is_none_or(|k| k == card);
    if no_piercing {
        // `if (bSize) { nPiercedSize++; } else if (IsVisPet()) { ... }`
        return !size && is_vis && fits(MAX_VIS, IK3_VIS);
    }
    if prop.item_kind3 == IK3_SUIT {
        fits(MAX_PIERCING_SUIT, IK3_SOCKETCARD)
    } else if prop.item_kind3 == IK3_SHIELD
        || prop.item_kind2 == IK2_WEAPON_DIRECT
        || prop.item_kind2 == IK2_WEAPON_MAGIC
    {
        fits(MAX_PIERCING_WEAPON, IK3_SOCKETCARD2)
    } else if is_vis {
        fits(MAX_VIS, IK3_VIS)
    } else {
        false
    }
}

/// Does the item's socket content count (`CMover::DestParamPiercingAvail`: `IsPierceAble()`)? Gear never does
/// with `features.no_piercing`; a vis pet always does.
pub fn cards_apply(prop: &ItemProp, pierced_size: usize, is_vis: bool, no_piercing: bool) -> bool {
    is_pierce_able(prop, pierced_size, is_vis, None, false, no_piercing)
}

/// `CItemElem::GetPiercingAvail` over the socketed `cards` and the piercing definitions of
/// `propItemEtc.inc`: `(DST, value)` summed per DST in order of first appearance. An empty socket or a card
/// without a definition adds nothing.
pub fn card_dsts(defs: &[PiercingDef], cards: &[u32]) -> Vec<(u32, i32)> {
    let mut out: Vec<(u32, i32)> = Vec::new();
    for &card in cards {
        let Some(def) = defs.iter().find(|d| d.item == card) else {
            continue;
        };
        for &[dst, adj] in &def.params {
            let dst = dst as u32;
            match out.iter_mut().find(|(d, _)| *d == dst) {
                Some(e) => e.1 = e.1.wrapping_add(adj),
                None => out.push((dst, adj)),
            }
        }
    }
    out
}

/// `CItemUpgrade::GetSizeProb`: the chance, out of 10 000, to add the socket `pierced_size + 1`. A suit reads
/// `suit_prob[size]`, a shield or weapon `weapon_prob[size]`; anything else (or a size past the table) is 0.
pub fn size_prob(
    prop: &ItemProp,
    pierced_size: usize,
    suit_prob: &[i32],
    weapon_prob: &[i32],
) -> i32 {
    let table = if prop.item_kind3 == IK3_SUIT {
        suit_prob
    } else if prop.item_kind3 == IK3_SHIELD
        || prop.item_kind2 == IK2_WEAPON_DIRECT
        || prop.item_kind2 == IK2_WEAPON_MAGIC
    {
        weapon_prob
    } else {
        return 0;
    };
    table.get(pierced_size).copied().unwrap_or(0)
}

/// The size roll: the attempt fails when `prob < roll` with `roll = xRandom(10000)` (so the true chance is
/// `(prob + 1) / 10000`, capped at 100 %).
pub fn size_roll_fails(prob: i32, roll: u32) -> bool {
    i64::from(prob) < i64::from(roll)
}

/// The index of the last filled socket of `cards` (`0` = empty), the one `PiercingRemove` clears.
pub fn last_filled(cards: &[u32]) -> Option<usize> {
    cards.iter().rposition(|&c| c != 0)
}

/// The index of the first empty socket of `cards` (`CMover::Pierce`).
pub fn first_empty(cards: &[u32]) -> Option<usize> {
    cards.iter().position(|&c| c == 0)
}
