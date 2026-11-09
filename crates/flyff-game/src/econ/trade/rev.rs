//! `PlayerItems::content_rev` (Plan/19 §8.4, Open question 7): what the trade watches to cancel itself when the
//! inventory of a participant changes underneath it.
//!
//! The plan describes a counter that every create, remove, count change and field update of an inventory item
//! bumps, while slot-only moves (swap, sort, equip), reservation changes and gold changes do not. The container
//! has many writers (`get_at_id_mut`, `iter_mut`, `add`, `remove_at_id`, `split`, `swap`, the sort of ITEM-15), so
//! instead of adding a counter to each of them this is a **digest of the item multiset**: an order-independent
//! sum of one hash per item over everything that makes an item what it is (serial, prop, count, flags, upgrade,
//! element, random option, keep time, sockets, pet) and over nothing that is only about where it sits or whether
//! it is offered (`reserved`, `vendor_price`, the storage entry, the slot). Two reads give the same value exactly
//! when the multiset is the same, which is what the cancellation rule asks for.

use crate::item::inventory::PlayerItems;
use crate::item::model::Item;

/// SplitMix64: a bijective mixer, so equal inputs give equal hashes and the sum cannot cancel structurally.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Folds a value into a running hash.
fn fold(h: u64, v: u64) -> u64 {
    mix(h ^ mix(v))
}

fn fold_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    h = fold(h, bytes.len() as u64);
    for chunk in bytes.chunks(8) {
        let mut word = [0_u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        h = fold(h, u64::from_le_bytes(word));
    }
    h
}

/// The hash of one item over its identity and content (never over `reserved`, `vendor_price` or the slot).
pub fn item_hash(it: &Item) -> u64 {
    let mut h = fold(0, u64::from(it.item_id));
    h = fold(h, it.serial.0);
    h = fold(h, u64::from(it.count));
    h = fold(h, u64::from(it.flags.0));
    h = fold(h, it.endurance as u64);
    h = fold(h, it.upgrade_level as u64);
    h = fold(h, it.element as u64);
    h = fold(h, it.element_level as u64);
    h = fold(h, u64::from(it.element_stone_id));
    h = fold(h, u64::from(it.guild_id));
    h = fold(h, u64::from(it.charged));
    h = fold(h, it.keep_until as u64);
    h = fold(h, it.random_option.0);
    h = fold(h, it.pending_random_option.map_or(u64::MAX, |w| w.0));
    h = fold(h, u64::from(it.collector_battery.unwrap_or(u32::MAX)));
    h = fold(h, u64::from(it.vis_transformed));
    h = fold_bytes(h, it.text.as_bytes());
    for c in &it.piercing.cards {
        h = fold(h, u64::from(*c));
    }
    h = fold(h, it.piercing.cards.len() as u64);
    for c in &it.piercing.ultimate_gems {
        h = fold(h, u64::from(*c));
    }
    h = fold(h, it.piercing.ultimate_gems.len() as u64);
    for t in &it.piercing.vis_keep_until {
        h = fold(h, *t as u64);
    }
    if let Some(p) = it.pet.as_deref() {
        h = fold(h, u64::from(p.kind));
        h = fold(h, p.level as u64);
        h = fold(h, u64::from(p.exp));
        h = fold(h, u64::from(p.energy));
        h = fold(h, u64::from(p.life));
        h = fold_bytes(h, p.name.as_bytes());
    }
    h
}

impl PlayerItems {
    /// The content revision of the inventory (bag and worn items): equal before and after exactly when no item was
    /// created, removed or changed; a slot-only move, a reservation or a wallet change keeps it (19 §8.4).
    pub fn content_rev(&self) -> u64 {
        self.inventory
            .iter()
            .fold(0_u64, |acc, (_, it)| acc.wrapping_add(item_hash(it)))
    }
}
