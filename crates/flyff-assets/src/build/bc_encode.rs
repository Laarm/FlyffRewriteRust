//! BC1/BC2/BC3 re-encoder for the mip levels >= 1 of the DDS fixes (Plan/24 §10.4.3, WP-WEB-09).
//!
//! The encoder is integer-only and deterministic (tier R). Per 4x4 block of RGBA8 texels (the blocks at
//! the right and bottom edge, and whole levels of 2x2 or 1x1 texels, replicate their edge texels):
//!
//! * **BC1.** If any texel has `a < 128` the block uses the 3-colour mode with transparency: the endpoints
//!   come from the opaque texels, `c0 <= c1`, the palette is `{c0, c1, (c0 + c1) / 2, transparent}` and a
//!   transparent texel gets index 3 (a block without opaque texel is `c0 = c1 = 0`, every index 3).
//!   Otherwise the 4-colour mode is used (`c0 >= c1`); a block whose endpoints are equal after the 565
//!   quantisation (all texels equal, or nearly) uses index 0 everywhere, so a solid colour that the 565
//!   grid can express round-trips exactly.
//! * **BC2.** 4-bit alpha `(a * 15 + 127) / 255` per texel, colour as the BC1 4-colour mode.
//! * **BC3.** alpha endpoints `a0 = max`, `a1 = min` (8-value mode when `a0 > a1`; equal gives index 0
//!   everywhere), 3-bit index = the nearest of the 8 interpolated values (ties to the lower index), colour
//!   as the BC1 4-colour mode.
//!
//! The colour endpoints are the per-channel minimum and maximum of the block, inset by `(max - min) / 16`
//! (integer division) and rounded to RGB565 with `(v * 31 + 127) / 255` (green `* 63`); the index is the
//! nearest palette entry by squared RGB distance, ties to the lower index. The palette uses the integer
//! formulas of the BC specification (`(2 * c0 + c1) / 3`, `(c0 + 2 * c1) / 3`, `(c0 + c1) / 2`, all
//! truncating), the same ones `flyff_formats::bc` decodes with, so encode and decode agree.

/// Bump on any change that alters the produced bytes (it moves the `dds` kind fingerprint).
pub const BC_ENCODE_VERSION: u32 = 1;

/// The block format to encode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BcFormat {
    /// DXT1, 8 bytes per block, 1-bit alpha.
    Bc1,
    /// DXT3, 16 bytes per block, explicit 4-bit alpha.
    Bc2,
    /// DXT5, 16 bytes per block, interpolated alpha.
    Bc3,
}

impl BcFormat {
    /// Bytes per 4x4 block.
    pub const fn block_bytes(self) -> usize {
        match self {
            BcFormat::Bc1 => 8,
            BcFormat::Bc2 | BcFormat::Bc3 => 16,
        }
    }

    /// Byte size of an encoded `w x h` level (`max(1, ceil(w / 4)) * max(1, ceil(h / 4)) * block`).
    pub fn level_size(self, w: u32, h: u32) -> usize {
        let blocks = (w.div_ceil(4).max(1) as usize).saturating_mul(h.div_ceil(4).max(1) as usize);
        blocks.saturating_mul(self.block_bytes())
    }
}

type Texels = [[u8; 4]; 16];

/// The 16 texels of block `(bx, by)`; coordinates beyond the image replicate the edge texel.
fn gather(rgba: &[u8], w: usize, h: usize, bx: usize, by: usize) -> Texels {
    let mut t = [[0u8; 4]; 16];
    for (i, slot) in t.iter_mut().enumerate() {
        let x = (bx * 4 + (i & 3)).min(w.saturating_sub(1));
        let y = (by * 4 + (i >> 2)).min(h.saturating_sub(1));
        let o = (y * w + x) * 4;
        if let Some(&[r, g, b, a]) = rgba
            .get(o..o + 4)
            .and_then(|s| <&[u8; 4]>::try_from(s).ok())
        {
            *slot = [r, g, b, a];
        }
    }
    t
}

/// RGB565 with rounding: `(v * 31 + 127) / 255` for red and blue, `(v * 63 + 127) / 255` for green.
fn quantise565(rgb: [u8; 3]) -> u16 {
    let [r, g, b] = rgb.map(u32::from);
    (((r * 31 + 127) / 255) << 11 | ((g * 63 + 127) / 255) << 5 | ((b * 31 + 127) / 255)) as u16
}

/// RGB565 to RGB888 by bit replication (what the BC decoders do).
fn expand565(c: u16) -> [u8; 3] {
    let (r, g, b) = (
        u32::from(c >> 11) & 31,
        u32::from(c >> 5) & 63,
        u32::from(c) & 31,
    );
    [
        ((r << 3) | (r >> 2)) as u8,
        ((g << 2) | (g >> 4)) as u8,
        ((b << 3) | (b >> 2)) as u8,
    ]
}

/// The colour palette of endpoints `c0`, `c1`: four colours (`four`) or three plus transparent.
fn palette(c0: u16, c1: u16, four: bool) -> [[u8; 3]; 4] {
    let (p0, p1) = (expand565(c0), expand565(c1));
    let mix = |a: u32, b: u32, wa: u32, wb: u32, d: u32| ((a * wa + b * wb) / d) as u8;
    let mut p2 = [0u8; 3];
    let mut p3 = [0u8; 3];
    for k in 0..3 {
        let (a, b) = (
            u32::from(p0.get(k).copied().unwrap_or(0)),
            u32::from(p1.get(k).copied().unwrap_or(0)),
        );
        if let (Some(s2), Some(s3)) = (p2.get_mut(k), p3.get_mut(k)) {
            if four {
                *s2 = mix(a, b, 2, 1, 3);
                *s3 = mix(a, b, 1, 2, 3);
            } else {
                *s2 = mix(a, b, 1, 1, 2);
                *s3 = 0;
            }
        }
    }
    [p0, p1, p2, p3]
}

fn dist2(a: [u8; 3], b: [u8; 3]) -> u32 {
    let mut d = 0u32;
    for (x, y) in a.into_iter().zip(b) {
        let e = u32::from(x.abs_diff(y));
        d += e * e;
    }
    d
}

/// The nearest of the first `count` palette entries (ties to the lower index).
fn nearest(pal: &[[u8; 3]; 4], count: usize, px: [u8; 3]) -> u32 {
    let (mut best, mut best_d) = (0u32, u32::MAX);
    for (i, p) in pal.iter().take(count).enumerate() {
        let d = dist2(*p, px);
        if d < best_d {
            best_d = d;
            best = i as u32;
        }
    }
    best
}

/// Per-channel minimum and maximum of the selected texels, inset by `(max - min) / 16`.
fn endpoints(t: &Texels, select: impl Fn(&[u8; 4]) -> bool) -> Option<([u8; 3], [u8; 3])> {
    let mut lo = [255u8; 3];
    let mut hi = [0u8; 3];
    let mut any = false;
    for px in t.iter().filter(|p| select(p)) {
        any = true;
        for k in 0..3 {
            if let (Some(l), Some(h), Some(&v)) = (lo.get_mut(k), hi.get_mut(k), px.get(k)) {
                *l = (*l).min(v);
                *h = (*h).max(v);
            }
        }
    }
    if !any {
        return None;
    }
    let mut lo_i = lo;
    let mut hi_i = hi;
    for k in 0..3 {
        let d = (hi.get(k).copied().unwrap_or(0) - lo.get(k).copied().unwrap_or(0)) / 16;
        if let (Some(l), Some(h)) = (lo_i.get_mut(k), hi_i.get_mut(k)) {
            *l += d;
            *h -= d;
        }
    }
    Some((lo_i, hi_i))
}

/// The 8-byte colour block. `bc1` allows the 3-colour mode with transparency; BC2/BC3 always use the
/// 4-colour palette.
fn colour_block(t: &Texels, bc1: bool) -> [u8; 8] {
    let transparent = bc1 && t.iter().any(|p| p[3] < 128);
    let (c0, c1, pal, count, flat) = if transparent {
        match endpoints(t, |p| p[3] >= 128) {
            // Only transparent texels: c0 = c1 = 0 (3-colour mode), every index 3.
            None => return [0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF],
            Some((lo, hi)) => {
                let (c0, c1) = (quantise565(lo), quantise565(hi));
                (c0, c1, palette(c0, c1, false), 3, c0 == c1)
            }
        }
    } else {
        let (lo, hi) = endpoints(t, |_| true).unwrap_or(([0; 3], [0; 3]));
        let (c0, c1) = (quantise565(hi), quantise565(lo));
        (c0, c1, palette(c0, c1, true), 4, c0 == c1)
    };
    let mut idx = 0u32;
    for (i, px) in t.iter().enumerate() {
        let v = if transparent && px[3] < 128 {
            3
        } else if flat {
            0
        } else {
            nearest(&pal, count, [px[0], px[1], px[2]])
        };
        idx |= v << (2 * i);
    }
    let [a, b] = c0.to_le_bytes();
    let [c, d] = c1.to_le_bytes();
    let [e, f, g, h] = idx.to_le_bytes();
    [a, b, c, d, e, f, g, h]
}

/// The 8-byte BC2 alpha block: `(a * 15 + 127) / 255`, texel `i` in byte `i / 2`, low nibble first.
fn bc2_alpha_block(t: &Texels) -> [u8; 8] {
    let mut out = [0u8; 8];
    for (i, px) in t.iter().enumerate() {
        let nibble = (u32::from(px[3]) * 15 + 127) / 255;
        if let Some(b) = out.get_mut(i / 2) {
            *b |= (nibble << (4 * (i & 1))) as u8;
        }
    }
    out
}

/// The 8-value BC3 alpha palette for `a0 > a1`.
fn bc3_alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (u32::from(a0), u32::from(a1));
    let mut p = [a0, a1, 0, 0, 0, 0, 0, 0];
    for i in 0..6u32 {
        if let Some(s) = p.get_mut(2 + i as usize) {
            *s = (((6 - i) * x + (1 + i) * y) / 7) as u8;
        }
    }
    p
}

/// The 8-byte BC3 alpha block (`a0 = max`, `a1 = min`).
fn bc3_alpha_block(t: &Texels) -> [u8; 8] {
    let (mut lo, mut hi) = (255u8, 0u8);
    for px in t {
        lo = lo.min(px[3]);
        hi = hi.max(px[3]);
    }
    if lo >= hi {
        return [hi, hi, 0, 0, 0, 0, 0, 0];
    }
    let pal = bc3_alpha_palette(hi, lo);
    let mut bits = 0u64;
    for (i, px) in t.iter().enumerate() {
        let (mut best, mut best_d) = (0u64, u32::MAX);
        for (k, &p) in pal.iter().enumerate() {
            let d = u32::from(p.abs_diff(px[3]));
            if d < best_d {
                best_d = d;
                best = k as u64;
            }
        }
        bits |= best << (3 * i);
    }
    let [i0, i1, i2, i3, i4, i5, _, _] = bits.to_le_bytes();
    [hi, lo, i0, i1, i2, i3, i4, i5]
}

/// Encodes a `w x h` RGBA8 image (top-down rows, `w * h * 4` bytes) as `format` blocks. Returns an empty
/// vector when a side is zero or `rgba` is shorter than the image needs.
pub fn encode(format: BcFormat, rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let Some(need) = wu.checked_mul(hu).and_then(|n| n.checked_mul(4)) else {
        return Vec::new();
    };
    if wu == 0 || hu == 0 || rgba.len() < need {
        return Vec::new();
    }
    let (bw, bh) = (wu.div_ceil(4), hu.div_ceil(4));
    let mut out = Vec::with_capacity(format.level_size(w, h));
    for by in 0..bh {
        for bx in 0..bw {
            let t = gather(rgba, wu, hu, bx, by);
            match format {
                BcFormat::Bc1 => {}
                BcFormat::Bc2 => out.extend_from_slice(&bc2_alpha_block(&t)),
                BcFormat::Bc3 => out.extend_from_slice(&bc3_alpha_block(&t)),
            }
            out.extend_from_slice(&colour_block(&t, format == BcFormat::Bc1));
        }
    }
    out
}
