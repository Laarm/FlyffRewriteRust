//! `[rates]` (Plan/03 §4.6; all class R): the global multipliers. `None` = the value of the data files
//! (`Constant.inc formula{}` for the EXP rate, 1.0 for the others). The struct is generated in
//! [`tables`](super::tables).

pub use super::tables::Rates;

impl Rates {
    /// `item_drop`, 1.0 when unset.
    pub fn item_drop_or_default(&self) -> f32 {
        self.item_drop.unwrap_or(1.0)
    }

    /// `gold_drop`, 1.0 when unset.
    pub fn gold_drop_or_default(&self) -> f32 {
        self.gold_drop.unwrap_or(1.0)
    }

    /// `monster_hit`, 1.0 when unset.
    pub fn monster_hit_or_default(&self) -> f32 {
        self.monster_hit.unwrap_or(1.0)
    }

    /// `shop_cost`, 1.0 when unset.
    pub fn shop_cost_or_default(&self) -> f32 {
        self.shop_cost.unwrap_or(1.0)
    }
}
