//! The arithmetic of the 1:1 guild combat (Plan/21 §8.3, §8.5; WP-SOCIAL-25, -26;
//! `src:_Common/GuildCombat1to1.cpp`): the refunds of a cancelled and of a failed tender and the red chips of a
//! round. Pure functions over plain numbers, bit-exact with the original x86 build (single precision in the order of
//! the C++ expression), so the browser and the server agree.
//!
//! The refund is **not** [`super::muldiv`]: the original multiplied a `float` (`(*it).nPenya * ((float)m_nCancelReturnRate
//! / 100)`), so a tender of 110 000 001 penya rounds to 110 000 000 as a `float` before the rate is applied.

/// `(int)` of a `float`: truncation toward zero, saturating (the original's range never leaves `int`); NaN is 0.
fn c_int(x: f32) -> i32 {
    if x.is_nan() { 0 } else { x as i32 }
}

/// The refund of a tender: `(int)( penya * ( (float)rate_percent / 100 ) )` in single precision
/// (`SetCancelTenderGuild` 1392 with `CancelReturnRate` 80, `SetFailedTenderGuild` 1445 with `FailReturnRate` 98).
///
/// `refund(110_000_001, 80) == 88_000_000`, `refund(110_000_001, 98) == 107_800_000`,
/// `refund(2_147_483_647, 98) == 2_104_534_016`, `refund(1, 80) == 0`.
pub fn refund(penya: i32, rate_percent: i32) -> i32 {
    let rate = rate_percent as f32 / 100.0_f32;
    c_int(penya as f32 * rate)
}

/// The red chips of a won round (`SetWinChip` 273-275): `(int)WinChipNum` of the script, at least 1.
///
/// `win_chips(90.00000000000001) == 90`, `win_chips(0.4) == 1`, `win_chips(f64::NAN) == 1`.
pub fn win_chips(win_chip_num: f64) -> i32 {
    // `as` saturates and maps NaN to 0, which the `max` then lifts to the minimum of one chip
    (win_chip_num as i32).max(1)
}
