//! The ultimate-weapon, piercing, transy and angel exports of `RulesContext` (Plan/19 §12.5, §12.7, §12.8, §5.12,
//! WP-ITEM-13, -14, -22): what the summon-angel, piercing, item-transy and ultimate-gem windows show, computed by
//! the same functions the server's handlers use (`flyff_rules::item::{angel, piercing, transy, ultimate}`).
//!
//! The Rust core ([`RulesCore`] methods) has no `JsValue` and is tested natively
//! (`tests/rules_upgrades_b.rs`); the `#[wasm_bindgen]` block at the end only converts. Nothing here reads
//! the probability tables of the server (`Ultimate_UltimateWeapon.txt`, `ItemUpgrade.lua`): they are not part of
//! the client bundles (Plan/19 OQ8).

use super::{RulesContext, RulesCore};
use flyff_rules::item::{angel, piercing, transy, ultimate};
use wasm_bindgen::prelude::wasm_bindgen;

/// What the browser passes for "any card kind" in [`RulesCore::pierce_able`] (the legacy `NULL_ID`).
pub const ANY_KIND: u32 = u32::MAX;

impl RulesCore {
    /// `angel_shares(pieces)`: the colour shares `[red, blue, green, white]` in percent for `pieces` Orichalcum and
    /// moonstones (`green = n`, `white = n / 10`, `blue = 2 n`, `red = 100 - ...`; red is negative above 32).
    pub fn angel_shares(&self, pieces: u32) -> [f32; 4] {
        angel::angel_shares(pieces)
    }

    /// `angel_odds(pieces)`: the exact chance of each colour in percent, counted over the 1000 values of the
    /// server's roll (`xRandom(1000) / 10` against the cumulative bounds), `[red, blue, green, white]`.
    pub fn angel_odds(&self, pieces: u32) -> [f32; 4] {
        let mut hist = [0_u32; 4];
        for tenths in 0..1000 {
            if let Some(slot) = angel::angel_bucket(pieces, tenths).and_then(|b| hist.get_mut(b)) {
                *slot += 1;
            }
        }
        hist.map(|n| n as f32 / 10.0)
    }

    /// `transy_scroll(item)`: the transy scroll (`II_CHR_SYS_SCR_ITEMTRANSY_A` up to level 60, `_B` above) an
    /// armour needs; `0` for an unknown item or one that no scroll can swap (no armour, no gender).
    pub fn transy_scroll(&self, item_id: u32) -> u32 {
        match self.items.get(item_id) {
            Some(p) if transy::is_swappable(p) => transy::scroll_for(p.limit_level1),
            _ => 0,
        }
    }

    /// `transy_twins(item)`: the items `item` can be swapped for (`RunItemTransy`'s test), ascending by id.
    pub fn transy_twins(&self, item_id: u32) -> Vec<u32> {
        let Some(from) = self.items.get(item_id) else {
            return Vec::new();
        };
        if !transy::is_swappable(from) {
            return Vec::new();
        }
        transy::twins(from, self.items.values())
            .map(|p| p.id)
            .collect()
    }

    /// `can_transy(from, to)`: `RunItemTransy`'s test for two items.
    pub fn can_transy(&self, from: u32, to: u32) -> bool {
        match (self.items.get(from), self.items.get(to)) {
            (Some(a), Some(b)) => transy::can_transy(a, b),
            _ => false,
        }
    }

    /// `pierce_able(item, pierced_size, card_kind3, size, vis_transformed, no_piercing)`: `IsPierceAble`. `card_kind3`
    /// is [`ANY_KIND`] for none; `size` asks about one more socket; `vis_transformed` is the item's
    /// `m_bTranformVisPet`; `no_piercing` is `features.no_piercing`. An unknown item is `false`.
    pub fn pierce_able(
        &self,
        item_id: u32,
        pierced_size: u32,
        card_kind3: u32,
        size: bool,
        vis_transformed: bool,
        no_piercing: bool,
    ) -> bool {
        let Some(p) = self.items.get(item_id) else {
            return false;
        };
        piercing::is_pierce_able(
            p,
            pierced_size as usize,
            p.is_vis_pet() || vis_transformed,
            (card_kind3 != ANY_KIND).then_some(card_kind3),
            size,
            no_piercing,
        )
    }

    /// `ultimate_gem_dsts(weapon, sockets)`: the `[dst, value]` pairs (flattened) the gems in `sockets` give an
    /// ultimate weapon, summed per DST in DST order; empty for any other item.
    pub fn ultimate_gem_dsts(&self, weapon_id: u32, sockets: &[u32]) -> Vec<i32> {
        let Some(weapon) = self.items.get(weapon_id) else {
            return Vec::new();
        };
        flyff_rules::item::tooltip::gem_dst_values(
            &self.items,
            &self.ext.gem_ability,
            weapon,
            sockets,
        )
        .into_iter()
        .flat_map(|(dst, value)| [dst as i32, value])
        .collect()
    }
}

/// `ultimate_sockets(ao)`: the gem sockets of a `+ao` ultimate weapon (`ao - 5`, none up to `+5`).
pub fn ultimate_sockets(ao: i32) -> u32 {
    ultimate::gem_sockets(ao) as u32
}

#[wasm_bindgen]
impl RulesContext {
    /// The colour shares `[red, blue, green, white]` in percent for `pieces` angel materials.
    pub fn angel_shares(&self, pieces: u32) -> Vec<f32> {
        self.core.angel_shares(pieces).to_vec()
    }

    /// The exact chance of each angel colour in percent, `[red, blue, green, white]`.
    pub fn angel_odds(&self, pieces: u32) -> Vec<f32> {
        self.core.angel_odds(pieces).to_vec()
    }

    /// The transy scroll an armour needs, `0` when no scroll can swap it.
    pub fn transy_scroll(&self, item_id: u32) -> u32 {
        self.core.transy_scroll(item_id)
    }

    /// The items an armour can be swapped for with a transy scroll, ascending by id.
    pub fn transy_twins(&self, item_id: u32) -> Vec<u32> {
        self.core.transy_twins(item_id)
    }

    /// `true` when `from` can be swapped for `to` with a transy scroll.
    pub fn can_transy(&self, from: u32, to: u32) -> bool {
        self.core.can_transy(from, to)
    }

    /// `IsPierceAble(card_kind3, size)` of an item (`cardKind3 = 0xFFFFFFFF` for "any", `noPiercing` is
    /// `features.no_piercing`).
    pub fn pierce_able(
        &self,
        item_id: u32,
        pierced_size: u32,
        card_kind3: u32,
        size: bool,
        vis_transformed: bool,
        no_piercing: bool,
    ) -> bool {
        self.core.pierce_able(
            item_id,
            pierced_size,
            card_kind3,
            size,
            vis_transformed,
            no_piercing,
        )
    }

    /// The `[dst, value, dst, value, ...]` the gems in `sockets` give an ultimate weapon.
    pub fn ultimate_gem_dsts(&self, weapon_id: u32, sockets: &[u32]) -> Vec<i32> {
        self.core.ultimate_gem_dsts(weapon_id, sockets)
    }

    /// The gem sockets of a `+ao` ultimate weapon.
    pub fn ultimate_sockets(&self, ao: i32) -> u32 {
        ultimate_sockets(ao)
    }
}
