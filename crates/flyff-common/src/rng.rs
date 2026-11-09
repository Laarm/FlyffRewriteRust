//! Random number generation for game logic (Plan/10 §4.8, PL-013, D2).
//!
//! * [`GameRng`] — the trait game code draws from. The three helpers carry the legacy `xRandom`/`xRandomF`
//!   semantics once, for every generator: [`GameRng::random_below`] = `xRandom(n)` (`next_u32() % n`),
//!   [`GameRng::random_range`] = `xRandom(min, max)`, [`GameRng::random_f32`] = `xRandomF(max)`,
//! * [`Pcg32Rng`] — seeded PCG32 (XSH-RR 64/32) for the per-world, service and per-Lua-state streams,
//! * [`LegacyLcg`] — the original `xRand()` generator (`g_next = g_next · 1103515245 + 12345`), only for
//!   the places where a legacy quirk depends on its exact sequence (KQ rows; none known at M0),
//! * the stream derivation helpers [`splitmix64`], [`stream_tag`], [`derive_seed`], [`world_stream`],
//!   [`named_stream`] and [`RNG_DERIVATION_VERSION`], shared by the game thread's `RngSet` (flyff-game)
//!   and the tests. Changing any derivation formula MUST bump [`RNG_DERIVATION_VERSION`].

use rand_core::Rng;
use rand_pcg::Pcg32;

/// Version of the stream derivation of §4.8 (recorded in the replay header; bump on any change of
/// [`splitmix64`], [`stream_tag`], [`derive_seed`], [`world_stream`] or [`named_stream`]).
pub const RNG_DERIVATION_VERSION: u32 = 1;

/// Source of randomness for game logic. Implementations must be fully deterministic for a given seed.
pub trait GameRng {
    /// Next raw 32-bit value (for [`LegacyLcg`]: the full 32-bit `g_next` of `xRand()`).
    fn next_u32(&mut self) -> u32;

    /// Uniform value in `[0, n)`: legacy `xRandom(n)` = `next_u32() % n`; `random_below(0) == 0`
    /// (the original divides by zero), PL-013.
    fn random_below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }

    /// Uniform value in `[a, b)` = `a + random_below(b − a)`; returns `a` when `b <= a` (legacy
    /// `xRandom(min, max)`).
    fn random_range(&mut self, a: u32, b: u32) -> u32 {
        if b > a {
            a + self.random_below(b - a)
        } else {
            a
        }
    }

    /// Uniform value in `[0, max)` (legacy `xRandomF`), computed from ONE `next_u32` as
    /// `(x >> 8) / 2^24 · max` (24 mantissa bits: identical on every platform). Returns `0.0` for a
    /// `max` that is not positive.
    fn random_f32(&mut self, max: f32) -> f32 {
        if max.is_nan() || max <= 0.0 {
            return 0.0;
        }
        let unit = (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0);
        let v = unit * max;
        if v < max {
            v
        } else {
            // Rounding reached `max` (large `max` with coarse f32 spacing): step to the previous float.
            f32::from_bits(max.to_bits().saturating_sub(1))
        }
    }
}

impl<R: GameRng + ?Sized> GameRng for &mut R {
    fn next_u32(&mut self) -> u32 {
        (**self).next_u32()
    }
}

/// The SplitMix64 mixer (Steele/Lea/Flood 2014 constants) of the value `x`: the output the standard
/// SplitMix64 generator produces for state `x` (golden-gamma increment included), so `splitmix64(0)` is
/// the well-known `0xE220A8397B1DCDAF` and a zero input does not collapse to zero.
pub const fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The stream tag of a named stream: seedless `xxh3_64` of the name (§4.8 catalogue).
pub fn stream_tag(name: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(name.as_bytes())
}

/// `splitmix64(seed ^ tag)`: the state seed of a stream derived from the game seed (`RngSet::derive`).
pub const fn derive_seed(seed: u64, tag: u64) -> u64 {
    splitmix64(seed ^ tag)
}

/// The stream of world `w`: `Pcg32Rng::new(splitmix64(seed ^ w), stream = w)` (§4.8; all layers of a
/// world share it).
pub fn world_stream(seed: u64, world: u32) -> Pcg32Rng {
    let w = u64::from(world);
    Pcg32Rng::new(splitmix64(seed ^ w), w)
}

/// A named stream (`service`, `event`, `lua_npc`, …): `Pcg32Rng::new(derive(tag), stream = tag)` with
/// `tag = stream_tag(name)` (§4.8).
pub fn named_stream(seed: u64, name: &str) -> Pcg32Rng {
    let tag = stream_tag(name);
    Pcg32Rng::new(derive_seed(seed, tag), tag)
}

/// Seeded PCG32 stream (`rand_pcg::Pcg32`, PCG-XSH-RR 64/32), the generator of all derived streams.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg32Rng(Pcg32);

impl Pcg32Rng {
    /// Creates the generator for `(state_seed, stream)`; the increment is `(stream << 1) | 1`, so
    /// different streams are independent sequences.
    pub fn new(state_seed: u64, stream: u64) -> Self {
        Self(Pcg32::new(state_seed, stream))
    }

    /// Next 64-bit value (two 32-bit draws).
    pub fn next_u64(&mut self) -> u64 {
        Rng::next_u64(&mut self.0)
    }
}

impl GameRng for Pcg32Rng {
    fn next_u32(&mut self) -> u32 {
        Rng::next_u32(&mut self.0)
    }
}

/// The original `xRand()` generator (`_Common/xUtil.h`): `g_next = g_next · 1103515245 + 12345` on a
/// `DWORD`. [`GameRng::random_below`] on it is exactly `xRandom(n) = xRand() % n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyLcg {
    next: u32,
}

impl LegacyLcg {
    /// `xSRand(seed)`.
    pub const fn new(seed: u32) -> Self {
        Self { next: seed }
    }

    /// `xRand()`: advances and returns the full 32-bit state.
    pub fn xrand(&mut self) -> u32 {
        self.next = self.next.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.next
    }
}

impl GameRng for LegacyLcg {
    fn next_u32(&mut self) -> u32 {
        self.xrand()
    }
}
