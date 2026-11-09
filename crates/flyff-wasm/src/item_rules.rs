//! Item rule functions for the browser (Plan/19 §22, §3.4, §3.5, §5.4, §5.12; WP-ITEM-15, WP-ITEM-20): FREE
//! `#[wasm_bindgen]` functions over `flyff_rules::item::buffitem`, so the pocket window, the inventory sort preview, the
//! EXP scroll tooltip and the angel window apply the very same rules as the server handlers.
//!
//! * [`pocket_capacity`]`(index, bigger_pocket) -> u32`: the slots of a pocket (`18 / 42 / 42`, vanilla `6 / 24 / 24`);
//! * [`pocket_available`]`(perm_extrabags, index, exists, expired) -> bool`;
//! * [`inventory_sort_order`]`(kinds) -> Uint32Array`: `kinds[i]` is the `item_kind3` of the i-th usable bag item in slot
//!   order; the answer lists the indices in the order `SortInventory` moves them (stable, PL-153);
//! * [`exp_scroll_plan`]`(scroll, level, taken_mask, buff_count, special_count) -> Int32Array` =
//!   `[code, slot, percent, max_tiers, band_low, band_high]` with `code` 0 apply (`slot` and `percent` are valid), 90 the
//!   level is outside the band of the scroll, 91 the tiers are full, 92 a special buff blocks every other scroll;
//!   `taken_mask` has bit `i` set when tier `i` holds a buff;
//! * [`angel_goal_exp`]`(exp1, percent) -> f64`: the angel EXP that `AngelBuff` needs (`exp1 / 100 * percent`, integer).
//!
//! Nothing here panics; every input is a value.

use flyff_rules::item::buffitem::{
    ExpScrollPlan, angel_goal, exp_scroll_plan as rules_exp_scroll_plan,
    pocket_available as rules_pocket_available, pocket_capacity as rules_pocket_capacity,
    sort_order,
};
use wasm_bindgen::prelude::wasm_bindgen;

/// `code` of [`exp_scroll_plan`]: the scroll works.
pub const EXP_APPLY: i32 = 0;
/// `code`: the level is outside the band of the scroll.
pub const EXP_WRONG_BAND: i32 = 90;
/// `code`: all tiers the level allows are taken.
pub const EXP_FULL: i32 = 91;
/// `code`: a special (`AMPESS`) buff blocks the other scrolls.
pub const EXP_BLOCKED: i32 = 92;

/// The core of [`exp_scroll_plan`] (natively testable).
pub fn exp_plan_row(
    scroll: u32,
    level: i32,
    taken_mask: u32,
    buff_count: i32,
    special_count: i32,
) -> [i32; 6] {
    let taken = [
        taken_mask & 1 != 0,
        taken_mask & 2 != 0,
        taken_mask & 4 != 0,
    ];
    match rules_exp_scroll_plan(scroll, level, taken, buff_count, special_count) {
        ExpScrollPlan::WrongBand { band } => [EXP_WRONG_BAND, -1, 0, 0, band.0, band.1],
        ExpScrollPlan::Limited => [EXP_BLOCKED, -1, 0, 0, 0, 0],
        ExpScrollPlan::Full { band, max_tiers } => [EXP_FULL, -1, 0, max_tiers, band.0, band.1],
        ExpScrollPlan::Apply {
            slot,
            percent,
            max_tiers,
        } => [
            EXP_APPLY,
            i32::try_from(slot).unwrap_or(0),
            percent,
            max_tiers,
            0,
            0,
        ],
    }
}

/// The core of [`inventory_sort_order`].
pub fn sort_indices(kinds: &[u32]) -> Vec<u32> {
    let items: Vec<(u16, u32)> = kinds
        .iter()
        .enumerate()
        .map(|(i, &k)| (u16::try_from(i).unwrap_or(u16::MAX), k))
        .collect();
    sort_order(&items).into_iter().map(u32::from).collect()
}

/// The slots of pocket `index`: `18 / 42 / 42` with `bigger_pocket`, else `6 / 24 / 24`; 0 for no such pocket.
#[wasm_bindgen]
pub fn pocket_capacity(index: u32, bigger_pocket: bool) -> u32 {
    rules_pocket_capacity(usize::try_from(index).unwrap_or(usize::MAX), bigger_pocket)
}

/// Whether pocket `index` can be used (`CPocketController::IsAvailable`).
#[wasm_bindgen]
pub fn pocket_available(perm_extrabags: bool, index: u32, exists: bool, expired: bool) -> bool {
    rules_pocket_available(
        perm_extrabags,
        usize::try_from(index).unwrap_or(usize::MAX),
        exists,
        expired,
    )
}

/// The order `SortInventory` moves the usable bag items in: indices into `kinds`, stable inside one kind.
#[wasm_bindgen]
pub fn inventory_sort_order(kinds: &[u32]) -> Vec<u32> {
    sort_indices(kinds)
}

/// `[code, slot, percent, max_tiers, band_low, band_high]` of the use of a banded EXP scroll (see the module
/// documentation).
#[wasm_bindgen]
pub fn exp_scroll_plan(
    scroll: u32,
    level: i32,
    taken_mask: u32,
    buff_count: i32,
    special_count: i32,
) -> Vec<i32> {
    exp_plan_row(scroll, level, taken_mask, buff_count, special_count).to_vec()
}

/// The angel EXP an `AngelBuff` request needs: `exp1 / 100 * percent` in integers (`percent` outside `1..=100` reads
/// as 100). `exp1` is an exact integer below 2^53.
#[wasm_bindgen]
pub fn angel_goal_exp(exp1: f64, percent: i32) -> f64 {
    angel_goal(exp1 as i64, percent) as f64
}
