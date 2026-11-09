//! The tax seam of the economy (Plan/19 §7.6, WP-ECON-01, WP-ECON-08; ref/13 §24, src:_Common/Tax.cpp:101-163).
//!
//! The shop code asks the hooks for the continent the buyer stands on and for the two rates of that
//! continent, books the tax it collected and asks whether an item is sold to the occupying guild only. The real
//! implementation is doc 22's `event::tax::TaxService` (`Services.event.tax`, WP-EVENT-12): the call sites of
//! `shop.rs` and `quick_sell.rs` read it through disjoint field borrows of `Services` (the former
//! `EconService.tax` box is gone). [`NoTax`] stays as the documented neutral implementation: every continent
//! answers `CONT_NODATA`, so nothing is taxed and nothing is occupation-only.
//!
//! "Taxable" is two conditions, evaluated by the *caller* so that a hook cannot get them wrong:
//! the buyer stands on a continent (`continent != CONT_NODATA`) and the item is not one of the four tax-exempt
//! items ([`flyff_rules::item::is_taxable_item`]). When both hold the caller passes the continent's rate to
//! the price functions of `flyff-rules`, otherwise `None`.

use crate::world::continent::CONT_NODATA;
use flyff_common::math::Vec3;
use flyff_common::{GuildId, WorldId};
use flyff_data::types::ItemId;

/// A continent id (`CONT_*`, `CONT_NODATA` = none).
pub type Continent = u8;

/// What a booked tax is for (`TAX_SALES 0`, `TAX_PURCHASE 1`, `TAX_ADMISSION 2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaxKind {
    /// Collected when a player sells to an NPC.
    Sales = 0,
    /// Collected when a player buys from an NPC.
    Purchase = 1,
    /// Collected at the Heaven Tower entrance only (the C++ has no secret-room call); the East and the lord entry
    /// have it, the West has none.
    Admission = 2,
}

/// The tax service as the economy sees it (Plan/19 §7.6). Implemented by doc 22's `event::tax::TaxService`.
pub trait TaxHooks: Send + std::fmt::Debug {
    /// `CTax::GetContinent` (= `CContinent::GetArea`): the area `pos` of `world` belongs to: `CONT_EAST`,
    /// `CONT_WEST` or [`CONT_NODATA`] when none.
    fn continent_of(&self, pos: Vec3, world: WorldId) -> Continent;

    /// `CTax::GetSalesTaxRate(continent)`: `nTaxRate * 0.1f * 0.1f`; 0.0 for [`CONT_NODATA`] and for a
    /// continent without tax information.
    fn sales_rate(&self, c: Continent) -> f32;

    /// `CTax::GetPurchaseTaxRate(continent)`.
    fn purchase_rate(&self, c: Continent) -> f32;

    /// `CTax::AddTax(continent, amount, kind)`: books `amount` (> 0) for the continent's owner.
    fn add_tax(&mut self, c: Continent, amount: i64, kind: TaxKind);

    /// `CTax::IsOccupationShopItem`: only the occupying guild may buy it (`AddOccupationShopItem` of `Tax.lua`).
    fn is_occupation_shop_item(&self, item: ItemId) -> bool;

    /// `CTax::IsOccupationGuildMember`: the buyer's `guild` is the one that occupies `c`, the continent the buyer
    /// stands on. [`CONT_NODATA`], a continent without an owner and a buyer without a guild answer `false` (never
    /// `None == None`).
    fn is_occupation_guild_member(&self, c: Continent, guild: Option<GuildId>) -> bool;
}

/// The M4 default: no continent has a tax, no item is occupation-only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoTax;

impl TaxHooks for NoTax {
    fn continent_of(&self, _pos: Vec3, _world: WorldId) -> Continent {
        CONT_NODATA
    }

    fn sales_rate(&self, _c: Continent) -> f32 {
        0.0
    }

    fn purchase_rate(&self, _c: Continent) -> f32 {
        0.0
    }

    fn add_tax(&mut self, _c: Continent, _amount: i64, _kind: TaxKind) {}

    fn is_occupation_shop_item(&self, _item: ItemId) -> bool {
        false
    }

    fn is_occupation_guild_member(&self, _c: Continent, _guild: Option<GuildId>) -> bool {
        false
    }
}

/// The rates one transaction pays: the continent and, when the item is taxable there, the rate. Built by
/// [`TaxQuote::of`] from the hooks, the buyer's position and the item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaxQuote {
    /// The continent the buyer stands on.
    pub continent: Continent,
    /// The purchase rate when the item is taxable there.
    pub purchase: Option<f32>,
    /// The sales rate when the item is taxable there.
    pub sales: Option<f32>,
}

impl TaxQuote {
    /// `IsApplyTaxRate` + the two rates for one item at one place.
    pub fn of(hooks: &dyn TaxHooks, pos: Vec3, world: WorldId, item: ItemId) -> TaxQuote {
        let continent = hooks.continent_of(pos, world);
        let taxable = continent != CONT_NODATA && flyff_rules::item::is_taxable_item(item);
        TaxQuote {
            continent,
            purchase: taxable.then(|| hooks.purchase_rate(continent)),
            sales: taxable.then(|| hooks.sales_rate(continent)),
        }
    }
}
