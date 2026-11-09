//! Set bonuses of the worn items (Plan/16 §13.5, ref/10 §8.1-8.2): the armour `+N` bonus and the named sets
//! of `propItemEtc.inc`.
//!
//! The legacy applied and removed them per equip event with a few order dependencies; the port computes the
//! wanted state from what is worn and replaces a source only when its membership changed, which gives the
//! same net effect (`ArmorSet`: all four of suit, gauntlet, boots and helmet worn and not expired, `n` = the
//! lowest upgrade level; `NamedSet{id}`: the sum of the bonuses whose piece count is reached).

use super::dst::DstSource;
use super::out::Out;
use super::source::{apply_source, remove_source};
use crate::item::env::ItemEnv;
use crate::mover::Mover;
use flyff_data::exp_table::SetItemAvail;
use flyff_rules::sets::{ARMOR_SET_PARTS, armor_set_dsts, named_set_dsts};

/// The four parts of the armour set bonus (`IsSetItemPart`).
pub const SET_PARTS: [u32; 4] = ARMOR_SET_PARTS;

/// The bonus level of the worn armour: the lowest upgrade level of the four set parts when all are worn and
/// none is expired (`GetSetItem`), else 0. The rule itself is `flyff_rules::sets::armor_set_level` (shared with
/// the client).
pub fn armor_set_level(m: &Mover) -> i32 {
    let Some(p) = m.player() else { return 0 };
    flyff_rules::sets::armor_set_level(SET_PARTS.map(|part| {
        p.items
            .inventory
            .get_equip(part)
            .map(|it| (it.upgrade_level, it.is_expired()))
    }))
}

/// `SetSetItemAvail(n)` as DST slots: hit rate, block (range and melee), max HP rate, the attack-power
/// column (`DST_ADDMAGIC` without `features.better_armor_bonus`) and the four base stats.
pub fn armor_set_slots(a: &SetItemAvail, better_armor_bonus: bool) -> Vec<(u32, i32, i32)> {
    armor_set_dsts(a, better_armor_bonus)
}

/// Brings the `ArmorSet` and `NamedSet` sources in line with the worn items.
pub fn refresh(m: &mut Mover, env: &ItemEnv<'_>, out: &mut Out) {
    let stats = env.stats();
    let data = env.data();
    // armour +N
    let want = armor_set_level(m);
    let have = m.player().map_or(0, |p| p.items.set_state.armor_n);
    if want != have {
        remove_source(m, stats, DstSource::ArmorSet, true, out);
        if want > 0
            && let Some(a) = stats.rules.set_item_avail(want)
        {
            let slots = armor_set_slots(a, stats.cfg.better_armor_bonus);
            apply_source(m, stats, DstSource::ArmorSet, &slots, true, out);
        }
        if let Some(p) = m.player_mut() {
            p.items.set_state.armor_n = want;
        }
    }
    // named sets
    for def in &data.item_etc.set_items {
        let id = u32::try_from(def.id).unwrap_or(0);
        let count = {
            let Some(p) = m.player() else { return };
            def.elems
                .iter()
                .filter(|e| {
                    u32::try_from(e.parts)
                        .ok()
                        .and_then(|part| p.items.inventory.get_equip(part))
                        .is_some_and(|it| it.item_id == e.item && !it.is_expired())
                })
                .count()
        };
        let have = m
            .player()
            .and_then(|p| p.items.set_state.named.get(&id).copied())
            .unwrap_or(0);
        if count == have {
            continue;
        }
        remove_source(m, stats, DstSource::NamedSet { set_id: id }, true, out);
        if count > 0 {
            let slots = named_set_dsts(&def.avail, count);
            apply_source(
                m,
                stats,
                DstSource::NamedSet { set_id: id },
                &slots,
                true,
                out,
            );
        }
        if let Some(p) = m.player_mut() {
            if count == 0 {
                p.items.set_state.named.remove(&id);
            } else {
                p.items.set_state.named.insert(id, count);
            }
        }
    }
}
