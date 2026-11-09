//! Element resistances of upgraded armour and the set-bonus helpers (Plan/16 §13.4, §13.5, §13.8;
//! WP-MOVER-15; ref/10 §8): the armour `+N` bonus, the named sets of `propItemEtc.inc` and the accessory
//! bonus lists as pure functions; `flyff_game::stats::equip_sets` applies them as DST sources.

use crate::consts::{NULL_CHGPARAM, dst::*};
use flyff_common::defs::attribute::{
    _EARTH, _ELECTRICITY, _FIRE, _WATER, _WIND, DST_RESIST_EARTH, DST_RESIST_ELECTRICITY,
    DST_RESIST_FIRE, DST_RESIST_WATER, DST_RESIST_WIND,
};
use flyff_common::defs::neuz::{PARTS_CAP, PARTS_FOOT, PARTS_HAND, PARTS_UPPER_BODY};
use flyff_data::exp_table::SetItemAvail;
use flyff_data::items_ext::accessory::{AccessoryTable, MAX_AAO};
use flyff_data::items_ext::item_etc::SetAvail;

/// `GetStrongDST(element)` (src:_Common/MoverEquip.cpp:86-138): the resistance an element grants.
/// Unknown elements answer `DST_RESIST_FIRE` like the original's default.
pub fn strong_dst(element: u8) -> u32 {
    match u32::from(element) {
        _FIRE => DST_RESIST_WIND,
        _WATER => DST_RESIST_FIRE,
        _WIND => DST_RESIST_EARTH,
        _ELECTRICITY => DST_RESIST_WATER,
        _EARTH => DST_RESIST_ELECTRICITY,
        _ => DST_RESIST_FIRE,
    }
}

/// `GetWeakDST(element)`: the resistance an element costs.
pub fn weak_dst(element: u8) -> u32 {
    match u32::from(element) {
        _FIRE => DST_RESIST_WATER,
        _WATER => DST_RESIST_ELECTRICITY,
        _WIND => DST_RESIST_FIRE,
        _ELECTRICITY => DST_RESIST_EARTH,
        _EARTH => DST_RESIST_WIND,
        _ => DST_RESIST_FIRE,
    }
}

/// `GetSelfDST(element)`: the resistance of the element itself (only the Thai build used it).
pub fn self_dst(element: u8) -> u32 {
    match u32::from(element) {
        _FIRE => DST_RESIST_FIRE,
        _WATER => DST_RESIST_WATER,
        _WIND => DST_RESIST_WIND,
        _ELECTRICITY => DST_RESIST_ELECTRICITY,
        _EARTH => DST_RESIST_EARTH,
        _ => DST_RESIST_FIRE,
    }
}

/// The two DST changes of an upgraded armour piece (`SetDestParamEquip`): `(weak, −2·level)` then
/// `(strong, +2·level)`. Empty for no element or level 0.
pub fn element_resist_dsts(element: u8, level: i32) -> [(u32, i32); 2] {
    [
        (weak_dst(element), level.wrapping_mul(-2)),
        (strong_dst(element), level.wrapping_mul(2)),
    ]
}

// ---- armour +N and named sets (16 §13.5) -----------------------------------------------------------------------

/// The four parts of the armour `+N` bonus (`IsSetItemPart`): suit, gauntlet, boots, helmet.
pub const ARMOR_SET_PARTS: [u32; 4] = [PARTS_UPPER_BODY, PARTS_HAND, PARTS_FOOT, PARTS_CAP];

/// `GetSetItem()`: the bonus level of the worn armour. `pieces` holds, for the parts of
/// [`ARMOR_SET_PARTS`] in that order, `Some((ability_option, expired))` for a worn item. All four must be
/// worn and none expired; the level is then the lowest `ability_option`, else 0 (so one expired part removes
/// the bonus).
pub fn armor_set_level(pieces: [Option<(i32, bool)>; 4]) -> i32 {
    let mut n = i32::MAX;
    for piece in pieces {
        match piece {
            Some((option, false)) => n = n.min(option),
            _ => return 0,
        }
    }
    n
}

/// `SetSetItemAvail(n)` as DST slots of one row of `Setitem` (zero entries are skipped): the hit rate
/// (`ADJ_HITRATE`, "Acc"), the block (`BLOCK_RANGE` and `BLOCK_MELEE`), `HP_MAX_RATE`, the fourth column
/// (`ATKPOWER_RATE` with `features.better_armor_bonus`, else `ADDMAGIC`) and the four base stats.
pub fn armor_set_dsts(a: &SetItemAvail, better_armor_bonus: bool) -> Vec<(u32, i32, i32)> {
    let mut out = Vec::new();
    if a.hit_rate != 0 {
        out.push((DST_ADJ_HITRATE, a.hit_rate, NULL_CHGPARAM));
    }
    if a.block != 0 {
        out.push((DST_BLOCK_RANGE, a.block, NULL_CHGPARAM));
        out.push((DST_BLOCK_MELEE, a.block, NULL_CHGPARAM));
    }
    if a.max_hit_point_rate != 0 {
        out.push((DST_HP_MAX_RATE, a.max_hit_point_rate, NULL_CHGPARAM));
    }
    if a.attack_power_rate != 0 {
        let dst = if better_armor_bonus {
            DST_ATKPOWER_RATE
        } else {
            DST_ADDMAGIC
        };
        out.push((dst, a.attack_power_rate, NULL_CHGPARAM));
    }
    if a.added != 0 {
        for dst in [DST_STR, DST_DEX, DST_INT, DST_STA] {
            out.push((dst, a.added, NULL_CHGPARAM));
        }
    }
    out
}

/// The bonuses of a named set (`propItemEtc.inc SetItem`) with `count` equipped, non-expired pieces: every
/// entry whose `n_equiped <= count`, in file order (identical to the legacy per-step add and remove).
pub fn named_set_dsts(avail: &[SetAvail], count: usize) -> Vec<(u32, i32, i32)> {
    avail
        .iter()
        .filter(|a| usize::try_from(a.equiped).is_ok_and(|n| n <= count))
        .map(|a| (a.dst as u32, a.adj, NULL_CHGPARAM))
        .collect()
}

/// `CAccessoryProperty::GetDst(item, level)` (16 §13.4 step 4): the `(dst, adj, NULL_CHGPARAM)` list of the
/// accessory at upgrade level `0..=20` (a level above 20 reads as 20, below 0 as 0); empty for an item that
/// is no accessory.
pub fn accessory_dsts(table: &AccessoryTable, item: u32, level: i32) -> Vec<(u32, i32, i32)> {
    let Some(def) = table.items.iter().find(|a| a.item == item) else {
        return Vec::new();
    };
    let level = usize::try_from(level.max(0)).unwrap_or(0).min(MAX_AAO);
    def.levels
        .get(level)
        .map(|pairs| {
            pairs
                .iter()
                .map(|p| (p[0] as u32, p[1], NULL_CHGPARAM))
                .collect()
        })
        .unwrap_or_default()
}

/// The client's armour set level (16 §13.8, `MVRF_SETITEMSFX`): the minimum of `option & 0xFF` over the four
/// armour parts of the `EquipInfo` list, 0 when a part is empty (`None`). For the local player the caller
/// passes the real upgrade levels. A level of 3 or more shows the set effect once.
pub fn armor_set_level_client(options: [Option<i32>; 4]) -> i32 {
    let mut n = i32::MAX;
    for o in options {
        match o {
            Some(v) => n = n.min(v & 0xFF),
            None => return 0,
        }
    }
    n
}
