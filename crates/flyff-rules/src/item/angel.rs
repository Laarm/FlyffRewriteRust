//! Angel creation odds (Plan/19 §5.12, ref/07c §6.1; src:WORLDSERVER/DPSrvr.cpp:9328-9494).
//!
//! `CreateAngel` takes up to 20 stacks of Orichalcum or moonstones. The number of material pieces `n` is the
//! green share in percent; white is a tenth of it, blue twice it, red the rest. The roll is
//! `xRandom(1000) / 10.0f` and the first bucket whose cumulative bound is `>=` the roll wins. Everything is
//! `f32` exactly like the original, so the browser's odds display and the server agree to the last bit.

use flyff_common::defs::item::{
    II_SYS_SYS_QUE_ANGEL_BLUE, II_SYS_SYS_QUE_ANGEL_GREEN, II_SYS_SYS_QUE_ANGEL_RED,
    II_SYS_SYS_QUE_ANGEL_WHITE,
};

/// `MAX_MATDIE`: the most distinct material stacks a request may name (VR-025).
pub const MAX_ANGEL_MATERIAL_ENTRIES: usize = 20;

/// The angel items in bucket order: red, blue, green, white.
pub const ANGEL_ITEMS: [u32; 4] = [
    II_SYS_SYS_QUE_ANGEL_RED,
    II_SYS_SYS_QUE_ANGEL_BLUE,
    II_SYS_SYS_QUE_ANGEL_GREEN,
    II_SYS_SYS_QUE_ANGEL_WHITE,
];

/// The share of each colour in percent for `pieces` material pieces: `[red, blue, green, white]`.
///
/// `green = n`, `white = green / 10`, `blue = green * 2`, `red = 100 - (white + green + blue)`. Red is
/// negative from 33 pieces on (the original allows it; the cumulative bounds then skip red).
pub fn angel_shares(pieces: u32) -> [f32; 4] {
    let green = pieces as f32;
    let white = green / 10.0;
    let blue = green * 2.0;
    let red = 100.0 - (white + green + blue);
    [red, blue, green, white]
}

/// The cumulative bounds `[red, red + blue, + green, + white]` of [`angel_shares`] (`fRate[4]`).
pub fn angel_bounds(pieces: u32) -> [f32; 4] {
    let [red, blue, green, white] = angel_shares(pieces);
    let b0 = red;
    let b1 = b0 + blue;
    let b2 = b1 + green;
    let b3 = b2 + white;
    [b0, b1, b2, b3]
}

/// The bucket (index into [`ANGEL_ITEMS`]) a roll of `tenths` (`xRandom(1000)`, `0..=999`) falls into: the
/// first whose bound is `>= tenths / 10.0f`. `None` only when every bound is below the roll (cannot happen
/// for `pieces <= 2^24`, the last bound is 100).
pub fn angel_bucket(pieces: u32, tenths: u32) -> Option<usize> {
    let roll = tenths as f32 / 10.0;
    angel_bounds(pieces).iter().position(|&bound| roll <= bound)
}

/// The angel item of a roll (see [`angel_bucket`]).
pub fn angel_item(pieces: u32, tenths: u32) -> Option<u32> {
    angel_bucket(pieces, tenths).and_then(|i| ANGEL_ITEMS.get(i).copied())
}
