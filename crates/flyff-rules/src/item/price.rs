//! Item prices (Plan/19 §7.3-§7.5, §7.7, §7.8, §11.4, §22; ref/13 §12, src:_Common/Item.cpp:123-145,
//! _Common/Mover.cpp:651-690, WORLDSERVER/DPSrvr.cpp:2838-3231, 13092-13217, _Common/post.cpp:61-85).
//!
//! Every function is pure and follows the original's number types (E7): `f32` where the C++ used `float`,
//! `f64` where it used `double`, `int` truncation exactly where the original cast. The server handlers and
//! the browser (shop windows, tooltips, the beauty-shop preview) call the same code.

use crate::stats::c_int;
use flyff_data::props::item::ItemProp;

/// `NULL_ID` as the `dwCost` of an item that cannot be priced.
const NULL_COST: u32 = 0xFFFF_FFFF;

/// `PERIN_VALUE`: the fixed NPC buy price of a Perin and its use value (`Resource/define.h:263`).
pub const PERIN_VALUE: i32 = 100_000_000;
/// `HAIR_COST` (`defineNeuz.h:1140`).
pub const HAIR_COST: i32 = 2_000_000;
/// `HAIRCOLOR_COST` (`defineNeuz.h:1141`).
pub const HAIRCOLOR_COST: i32 = 4_000_000;
/// `CHANGE_FACE_COST` (`defineNeuz.h:1142`).
pub const CHANGE_FACE_COST: i32 = 1_000_000;

/// `CItemBase::GetCost` (src:_Common/Item.cpp:123-145): `-1` when the prop has no price (`dwCost == NULL_ID`,
/// such items are not for sale), else the cost, raised for an upgraded item by
/// `cost * (0.1 + level^2 / 18)` (all in `f32`, then truncated).
pub fn get_cost(prop: &ItemProp, upgrade_level: i32) -> i32 {
    if prop.cost == NULL_COST {
        return -1;
    }
    if upgrade_level != 0 {
        let c = prop.cost as f32;
        let ao = upgrade_level.wrapping_mul(upgrade_level) as f32;
        c_int(c + c * (0.1_f32 + ao / 18.0_f32))
    } else {
        prop.cost as i32
    }
}

/// The item part of `CTax::IsApplyTaxRate` (src:_Common/Tax.cpp:149-163, Plan/19 §7.6): every item pays the
/// continent's tax except the Perin, the Red Chip and the two awakening scrolls. The other half of the rule
/// (the player stands on a continent, `CONT_NODATA` pays nothing) is the caller's: it passes no rate.
pub fn is_taxable_item(item_id: u32) -> bool {
    use flyff_common::defs::item::{
        II_CHP_RED, II_SYS_SYS_SCR_AWAKE, II_SYS_SYS_SCR_PERIN, II_SYS_SYS_SCR_PETAWAKE,
    };
    !matches!(
        item_id,
        II_SYS_SYS_SCR_PERIN | II_CHP_RED | II_SYS_SYS_SCR_AWAKE | II_SYS_SYS_SCR_PETAWAKE
    )
}

/// `CItemBase::GetChipCost`: the Red Chip price (`refer_value1`), `-1` when absent.
pub fn chip_cost(prop: &ItemProp) -> i32 {
    if prop.refer_value1 == NULL_COST {
        -1
    } else {
        prop.refer_value1 as i32
    }
}

/// The unit price of a purchase from an NPC shop (Plan/19 §7.3 step 4, before the quantity is limited).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuyPrice {
    /// The price of one piece before tax, at least 1 (`cost`).
    pub unit: i32,
    /// The tax on one piece (0 when the item is not taxable).
    pub tax: i32,
}

/// `buy_unit_price`: `cost` is [`get_cost`] of the stock item. The steps, each truncating to `int`:
/// `shop_rate` (`rates.shop_cost`, `None` = 1.0), then the event factor (`1.0` while `EventFunc.lua` has no
/// buy function), the fixed Perin price, a floor of 1, and the purchase tax `(unit * rate) as int` for taxable
/// items (`purchase_rate = None` = not taxable). The handler limits the quantity with `unit` **before** the
/// tax is added (KEEP-QUIRK: a purchase can then fail silently on the total).
pub fn buy_unit_price(
    cost: i32,
    shop_rate: Option<f32>,
    event_factor: f32,
    is_perin: bool,
    purchase_rate: Option<f32>,
) -> BuyPrice {
    let mut unit = c_int(shop_rate.unwrap_or(1.0) * cost as f32);
    unit = c_int(event_factor * unit as f32);
    if is_perin {
        unit = PERIN_VALUE;
    }
    if unit < 1 {
        unit = 1;
    }
    let tax = purchase_rate.map_or(0, |r| c_int(unit as f32 * r));
    BuyPrice { unit, tax }
}

/// The price of one piece sold to an NPC (Plan/19 §7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SellPrice {
    /// What the player receives for one piece (after the tax, possibly 0 or negative for absurd rates; the
    /// handler floors the total at 1).
    pub unit: i32,
    /// The tax withheld per piece.
    pub tax: i32,
}

/// `sell_unit_price`: a quarter of [`get_cost`] (C integer division, so an unpriced item gives 0), times the
/// event factor, at least 1, minus the sales tax `(unit * rate) as int` (`sales_rate = None` = not taxable).
pub fn sell_unit_price(cost: i32, event_factor: f32, sales_rate: Option<f32>) -> SellPrice {
    let mut unit = cost / 4;
    unit = c_int(event_factor * unit as f32);
    if unit == 0 {
        unit = 1;
    }
    let tax = sales_rate.map_or(0, |r| c_int(unit as f32 * r));
    SellPrice {
        unit: unit - tax,
        tax,
    }
}

/// `quick_sell_unit_price` (Plan/19 §7.7 step 2): `max(cost / 4, 1)`; the tax applies only when the caller
/// passes a rate (taxable item and a GENERAL-authority seller); a bound item pays 1 penya, but the tax is
/// still credited (KEEP, KQ-15).
pub fn quick_sell_unit_price(cost: i32, sales_rate: Option<f32>, binds: bool) -> SellPrice {
    let mut unit = (cost / 4).max(1);
    let tax = sales_rate.map_or(0, |r| c_int(unit as f32 * r));
    unit -= tax;
    if binds {
        unit = 1;
    }
    SellPrice { unit, tax }
}

/// `MAX_KEEP_MAX_DAY`: a mail is deleted after 15 days; the storage fee rises over the last week.
const KEEP_DAYS: f64 = 15.0;

/// The fee for taking an item out of a mail (Plan/19 §11.4 step 2, PL-073: computed from full timestamps).
/// `cost` is the item's [`get_cost`], `elapsed_s` the age of the mail. The fee is 0 for the first week and
/// then grows by an eighth of the price per day.
pub fn mail_storage_fee(cost: i32, elapsed_s: i64) -> i32 {
    let elapsed = elapsed_s.max(0) as f64;
    let days = elapsed / 86_400.0;
    let hours = elapsed / 3_600.0;
    let n_day = (16.0 - days) as i32;
    let keep_h = (360.0 - hours).max(0.0) as u32;
    // 360 hours = KEEP_DAYS * 24
    let _ = KEEP_DAYS;
    if 360_u32.saturating_sub(keep_h) > 168 {
        let custody = (8_i32.wrapping_sub(n_day)) as f32 / 8.0_f32;
        let pay = (cost as f32 * custody).max(0.0);
        c_int(pay)
    } else {
        0
    }
}

/// `CMover::GetHairCost` (src:_Common/Mover.cpp:651-690): the current look is `cur_mesh` and the stored
/// colour `cur_rgb` (bytes). The stored colour is first converted through the original's float path
/// (`byte / 255.0f * 255`, truncated, so some bytes change by one), then compared with the requested one.
pub fn hair_cost(cur_mesh: u32, cur_rgb: [u8; 3], rgb: [u8; 3], hair: u8) -> i32 {
    let original = cur_rgb.map(hair_color_roundtrip);
    let color = if rgb != original { HAIRCOLOR_COST } else { 0 };
    // `m_dwHairMesh + 1 != nHair + 1`
    let mesh = if cur_mesh.wrapping_add(1) != u32::from(hair) + 1 {
        HAIR_COST
    } else {
        0
    };
    color + mesh
}

/// `(BYTE)(m_fHairColorR * 255)` with `m_fHairColorR = byte / 255.f`.
pub fn hair_color_roundtrip(byte: u8) -> u8 {
    ((f32::from(byte) / 255.0_f32) * 255.0_f32) as u8
}
