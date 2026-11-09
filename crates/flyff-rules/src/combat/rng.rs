//! RNG wrappers, `MulDiv` and the charge multipliers (Plan/17 §1.3, §2.6; WP-COMBAT-01).
//!
//! Every draw goes through the injected [`GameRng`] (PL-013). The wrappers keep the original call shapes:
//! `xRandom(n)` is `random_below` (`n == 0` answers 0 where the original divides by zero),
//! `xRandom(min, max)` is `[min, max)` and returns `min` when `max <= min`.

use flyff_common::rng::GameRng;

/// `xRandom(n)` (`_Common/xUtil.h`): `[0, n)`; `n == 0` gives 0.
pub fn x_random(rng: &mut dyn GameRng, n: u32) -> u32 {
    rng.random_below(n)
}

/// `xRandom(min, max)` (`xUtil.cpp`): `[a, b)` and `a` when `b <= a`. The original takes `DWORD`s, so the
/// comparison is **unsigned** (KEEP-QUIRK): with a negative `a` and a `b` below zero's wrap `a` comes back
/// unchanged, and the sum wraps like the `DWORD` it is.
pub fn x_random_range(rng: &mut dyn GameRng, a: i32, b: i32) -> i32 {
    rng.random_range(a as u32, b as u32) as i32
}

/// Win32 `MulDiv(a, b, c)`: the 64-bit product `a * b` divided by `c`, rounded to the nearest integer with halves
/// away from zero; `c == 0` or a result outside the `int` range gives `-1` (`MoverAttack.cpp` uses it for the
/// element factor and the defense factor).
pub fn mul_div(a: i32, b: i32, c: i32) -> i32 {
    if c == 0 {
        return -1;
    }
    let negative = (a < 0) ^ (b < 0) ^ (c < 0);
    let num = i64::from(a).unsigned_abs() * i64::from(b).unsigned_abs();
    let den = i64::from(c).unsigned_abs();
    let q = (num + den / 2) / den;
    if q > i32::MAX as u64 {
        return -1;
    }
    let q = q as i32;
    if negative { -q } else { q }
}

/// `GetChargeMultiplier` table: the bow charge multiplier by level `0..=4` (`MoverAttack.cpp:88-98`).
pub const BOW_CHARGE: [f32; 5] = [1.0, 1.2, 1.5, 1.8, 2.2];

/// The vanilla wand charge table (`GetWandATKMultiplier` without `__WAND_AUTOATTACKS`, `MoverAttack.cpp:101-116`).
pub const WAND_CHARGE: [f32; 5] = [0.6, 0.8, 1.05, 1.1, 1.3];

/// `GetChargeMultiplier(level)`: [`BOW_CHARGE`]`[level]`, 1.0 outside `0..=4`.
pub fn bow_charge_multiplier(level: u32) -> f32 {
    BOW_CHARGE.get(level as usize).copied().unwrap_or(1.0)
}

/// `GetWandATKMultiplier(level)`: 1.0 for every level with `features.wand_autoattacks`, otherwise
/// [`WAND_CHARGE`]`[level]` (1.0 outside `0..=4`).
pub fn wand_charge_multiplier(level: u32, wand_autoattacks: bool) -> f32 {
    if wand_autoattacks {
        1.0
    } else {
        WAND_CHARGE.get(level as usize).copied().unwrap_or(1.0)
    }
}
