//! The determinism self-test (Plan/02 §9.5, WP-CORE-06): a fixed computation over `libm` and `f32`/`f64`
//! arithmetic whose 64-bit hash must be identical natively (dev and release), under Node and in the
//! browser. It proves that the toolchain does not fuse multiply-adds, reorder float operations or
//! swap in platform math, which is the premise of the shared simulation (D4).
//!
//! Rules (tier S): no SIMD, no `mul_add`, no `powi`, transcendental functions only through `libm`.

/// FNV-1a over the little-endian bytes of every value fed in.
struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Fnv {
        Fnv(Self::OFFSET)
    }

    fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn f32(&mut self, v: f32) {
        self.u64(u64::from(v.to_bits()));
    }

    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
}

/// splitmix64: integer-only, used to derive the inputs from the seed.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Maps 24 random bits to an `f32` in `[0, 1)` exactly.
fn unit_f32(r: u64) -> f32 {
    let top = u32::try_from(r >> 40).unwrap_or(0); // 24 bits
    top as f32 / 16_777_216.0
}

/// Maps 53 random bits to an `f64` in `[0, 1)` exactly.
fn unit_f64(r: u64) -> f64 {
    (r >> 11) as f64 / 9_007_199_254_740_992.0
}

/// Hash of a fixed `libm`/`f32`/`f64` computation sequence derived from `seed` (256 rounds).
pub fn selftest_det(seed: u32) -> u64 {
    let mut state = u64::from(seed) ^ 0x5eed_0000_0000_0000;
    let mut h = Fnv::new();
    h.u64(u64::from(seed));
    let mut acc32: f32 = 1.0 + unit_f32(splitmix(&mut state));
    let mut acc64: f64 = 1.0 + unit_f64(splitmix(&mut state));
    for i in 0..256u32 {
        let a = unit_f32(splitmix(&mut state)) * 6.0 - 3.0;
        let b = unit_f32(splitmix(&mut state)) * 2.0 + 0.25;
        let c = unit_f64(splitmix(&mut state)) * 4.0 - 2.0;
        // f32 operations the simulation relies on
        let s = libm::sinf(a * acc32);
        let co = libm::cosf(a + b);
        let at = libm::atan2f(s, co + 2.0);
        let sq = libm::sqrtf(b * b + s * s);
        let ex = libm::expf(-b);
        let lg = libm::logf(b + 1.0);
        let pw = libm::powf(b, 1.5);
        let fl = libm::floorf(a * 7.0);
        acc32 = (acc32 * 0.5 + (s * co + at - sq) * 0.25 + (ex - lg) * 0.125 + pw * 0.0625) / 1.5
            + fl * 0.001953125
            + 1.0;
        // f64 operations (GAMETIMER, large counters)
        let d = libm::sin(c * acc64);
        let e = libm::exp(c * 0.25);
        let f = libm::fmod(c * 1000.0 + acc64, 7.5);
        acc64 = (acc64 * 0.5 + d * e + f * 0.125) / 1.25 + 1.0;
        // mixed integer <-> float conversions, saturating casts and bit patterns
        let as_i = (acc32 * 1000.0) as i32;
        let as_u = (acc64 * 4096.0) as u32;
        h.f32(acc32);
        h.f64(acc64);
        h.u64(u64::from(as_u) ^ (u64::from(as_i as u32) << 32));
        h.u64(u64::from(i));
    }
    h.f32(acc32);
    h.f64(acc64);
    h.0
}
