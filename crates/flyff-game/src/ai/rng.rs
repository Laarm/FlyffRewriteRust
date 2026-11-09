//! The random helpers of the AI (Plan/18 §8.5, PL-013): `x_random(n)` and `x_random_f(f)` over the world stream
//! that the tick loop hands to every handler as `AiCx.rng`. AI code never builds a generator of its own.

use flyff_common::rng::GameRng;

/// The raw 32-bit draw (legacy `xRand()`).
#[inline]
pub fn x_rand(rng: &mut dyn GameRng) -> u32 {
    rng.next_u32()
}

/// `xRandom(n)` = a draw in `[0, n)`; `x_random(0) == 0` (PL-013: the original divides by zero).
#[inline]
pub fn x_random(rng: &mut dyn GameRng, n: u32) -> u32 {
    rng.random_below(n)
}

/// `xRandomF(f)` = a draw in `[0, f)` from one `next_u32` (`GameRng::random_f32`).
#[inline]
pub fn x_random_f(rng: &mut dyn GameRng, f: f32) -> f32 {
    rng.random_f32(f)
}
