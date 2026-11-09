//! BC1/2/3 (DXT1/3/5) block codec (Plan/30 §11.2 decode, §11.6 range-fit encode).
//!
//! Both directions are integer-only and always compiled (native DDS fixes and the browser's S3TC-less
//! fallback use them), so the bytes are identical natively and under wasm. The decoder output is the
//! specification: GPUs may differ by +-1 per channel (D3D10 tolerance) and the goldens
//! (`tests/golden/bc-vectors.json`) are defined by §11.2 only.
//!
//! * Decode: RGBA8, top-down rows, `w * h * 4` bytes. Texels beyond the image edge (partial blocks) are not
//!   written. A BC1 block with `c0 <= c1` has 3 colours + transparent black; BC2/BC3 colour blocks always use
//!   the 4-colour mode.
//! * Encode (range fit): per block the per-channel `max`/`min` become the endpoints; no cluster fit. It is
//!   good enough for the lower mip levels `complete_mips` generates and for synthetic test textures.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError};
use crate::image::{array_at, rgba_len};

/// 16 RGBA texels of one block, texel `(x, y)` at index `4 * y + x`.
type Texels = [[u8; 4]; 16];

/// Which block layout.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bc1,
    Bc2,
    Bc3,
}

impl Kind {
    fn block_bytes(self) -> usize {
        match self {
            Kind::Bc1 => 8,
            Kind::Bc2 | Kind::Bc3 => 16,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------------------------------------

/// RGB565 to 8-bit channels by bit replication (`r = (r5 << 3) | (r5 >> 2)` etc.).
#[inline]
fn expand565(c: u16) -> [u32; 3] {
    let r5 = u32::from(c >> 11) & 31;
    let g6 = u32::from(c >> 5) & 63;
    let b5 = u32::from(c) & 31;
    [
        (r5 << 3) | (r5 >> 2),
        (g6 << 2) | (g6 >> 4),
        (b5 << 3) | (b5 >> 2),
    ]
}

/// The 4-entry colour palette (RGBA). `four_colour`: `[p0, p1, (2p0+p1)/3, (p0+2p1)/3]`; otherwise
/// `[p0, p1, (p0+p1)/2, transparent black]`. Integer division truncates.
#[inline]
fn colour_palette(c0: u16, c1: u16, four_colour: bool) -> [[u8; 4]; 4] {
    let [r0, g0, b0] = expand565(c0);
    let [r1, g1, b1] = expand565(c1);
    let px = |r: u32, g: u32, b: u32| [r as u8, g as u8, b as u8, 255];
    if four_colour {
        [
            px(r0, g0, b0),
            px(r1, g1, b1),
            px((2 * r0 + r1) / 3, (2 * g0 + g1) / 3, (2 * b0 + b1) / 3),
            px((r0 + 2 * r1) / 3, (g0 + 2 * g1) / 3, (b0 + 2 * b1) / 3),
        ]
    } else {
        [
            px(r0, g0, b0),
            px(r1, g1, b1),
            px((r0 + r1) / 2, (g0 + g1) / 2, (b0 + b1) / 2),
            [0, 0, 0, 0],
        ]
    }
}

#[inline]
fn pick4(p: &[[u8; 4]; 4], i: u32) -> [u8; 4] {
    let [a, b, c, d] = *p;
    match i & 3 {
        0 => a,
        1 => b,
        2 => c,
        _ => d,
    }
}

/// Decodes the 8-byte colour block. `force_four`: BC2/BC3 (always the 4-colour mode).
#[inline]
fn decode_colour(cb: [u8; 8], force_four: bool) -> Texels {
    let [a, b, c, d, e, f, g, h] = cb;
    let c0 = u16::from_le_bytes([a, b]);
    let c1 = u16::from_le_bytes([c, d]);
    let idx = u32::from_le_bytes([e, f, g, h]);
    let pal = colour_palette(c0, c1, force_four || c0 > c1);
    let mut out = [[0u8; 4]; 16];
    for (i, t) in out.iter_mut().enumerate() {
        *t = pick4(&pal, idx >> (2 * i));
    }
    out
}

/// BC3 alpha palette: 8 values for `a0 > a1`, else 6 values + 0 + 255.
#[inline]
fn bc3_alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (u32::from(a0), u32::from(a1));
    if a0 > a1 {
        [
            a0,
            a1,
            ((6 * x + y) / 7) as u8,
            ((5 * x + 2 * y) / 7) as u8,
            ((4 * x + 3 * y) / 7) as u8,
            ((3 * x + 4 * y) / 7) as u8,
            ((2 * x + 5 * y) / 7) as u8,
            ((x + 6 * y) / 7) as u8,
        ]
    } else {
        [
            a0,
            a1,
            ((4 * x + y) / 5) as u8,
            ((3 * x + 2 * y) / 5) as u8,
            ((2 * x + 3 * y) / 5) as u8,
            ((x + 4 * y) / 5) as u8,
            0,
            255,
        ]
    }
}

#[inline]
fn pick8(p: &[u8; 8], i: u64) -> u8 {
    p.get((i & 7) as usize).copied().unwrap_or(0)
}

/// Decodes one block of `kind` (`block` is exactly `kind.block_bytes()` long).
#[inline]
fn decode_block(kind: Kind, block: &[u8]) -> Texels {
    match kind {
        Kind::Bc1 => match array_at::<8>(block, 0) {
            Some(cb) => decode_colour(cb, false),
            None => [[0; 4]; 16],
        },
        Kind::Bc2 | Kind::Bc3 => {
            let (Some(ab), Some(cb)) = (array_at::<8>(block, 0), array_at::<8>(block, 8)) else {
                return [[0; 4]; 16];
            };
            let mut out = decode_colour(cb, true);
            if kind == Kind::Bc2 {
                // 4 bits per texel, texel i in byte i / 2, low nibble first.
                let nibbles = ab.iter().flat_map(|&b| [b & 15, b >> 4]);
                for (t, n) in out.iter_mut().zip(nibbles) {
                    t[3] = n * 17;
                }
            } else {
                let [a0, a1, i0, i1, i2, i3, i4, i5] = ab;
                let pal = bc3_alpha_palette(a0, a1);
                let bits = u64::from_le_bytes([i0, i1, i2, i3, i4, i5, 0, 0]);
                for (i, t) in out.iter_mut().enumerate() {
                    t[3] = pick8(&pal, bits >> (3 * i));
                }
            }
            out
        }
    }
}

fn decode(
    kind: Kind,
    blocks: &[u8],
    width: u32,
    height: u32,
    out: &mut [u8],
) -> Result<(), FormatError> {
    let need_out = rgba_len(FormatKind::Dds, 0, width, height)?;
    let (w, h) = (width as usize, height as usize);
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let bb = kind.block_bytes();
    let need_in = bw.saturating_mul(bh).saturating_mul(bb);
    if blocks.len() < need_in {
        return Err(FormatError::new(
            FormatKind::Dds,
            blocks.len(),
            ErrorKind::UnexpectedEof {
                needed: need_in as u64,
                remaining: blocks.len() as u64,
            },
        ));
    }
    let out_len = out.len();
    let Some(out) = out.get_mut(..need_out) else {
        return Err(FormatError::new(
            FormatKind::Dds,
            0,
            ErrorKind::InvalidValue {
                field: "output length",
                value: out_len as i64,
            },
        ));
    };
    let stride = w * 4;
    for (band, row_blocks) in out.chunks_mut(stride * 4).zip(blocks.chunks_exact(bw * bb)) {
        for (bx, block) in row_blocks.chunks_exact(bb).enumerate() {
            let texels = decode_block(kind, block);
            let x0 = bx * 4;
            let n = (w - x0).min(4);
            for (row, src) in band.chunks_exact_mut(stride).zip(texels.as_chunks::<4>().0) {
                if let Some(dst) = row.get_mut(x0 * 4..) {
                    for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src).take(n) {
                        d.copy_from_slice(s);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Decodes BC1 (DXT1) blocks into RGBA8 (`width * height * 4` bytes, top-down; `out` may be longer, only
/// the prefix is written). Partial edge blocks write only in-bounds texels. Errors: `UnexpectedEof` when
/// `blocks` is shorter than `ceil(w/4) * ceil(h/4) * 8`, `InvalidValue` for a zero side or a short `out`,
/// `DimensionTooLarge` above the decode caps (8192 per side, 16 Mi texels).
pub fn decode_bc1(blocks: &[u8], w: u32, h: u32, out: &mut [u8]) -> Result<(), FormatError> {
    decode(Kind::Bc1, blocks, w, h, out)
}

/// Decodes BC2 (DXT3, explicit 4-bit alpha) blocks; see [`decode_bc1`].
pub fn decode_bc2(blocks: &[u8], w: u32, h: u32, out: &mut [u8]) -> Result<(), FormatError> {
    decode(Kind::Bc2, blocks, w, h, out)
}

/// Decodes BC3 (DXT5, interpolated alpha) blocks; see [`decode_bc1`].
pub fn decode_bc3(blocks: &[u8], w: u32, h: u32, out: &mut [u8]) -> Result<(), FormatError> {
    decode(Kind::Bc3, blocks, w, h, out)
}

// ---------------------------------------------------------------------------------------------------------
// Encode (range fit)
// ---------------------------------------------------------------------------------------------------------

/// Gathers the 4x4 block at `(bx, by)`; bit `i` of the returned mask is set for in-bounds texels.
fn gather(rgba: &[u8], w: usize, h: usize, bx: usize, by: usize) -> (Texels, u16) {
    let mut t = [[0u8; 4]; 16];
    let mut valid = 0u16;
    for y in 0..4 {
        let py = by * 4 + y;
        if py >= h {
            break;
        }
        for x in 0..4 {
            let px = bx * 4 + x;
            if px >= w {
                break;
            }
            let i = y * 4 + x;
            if let (Some(p), Some(slot)) = (array_at::<4>(rgba, (py * w + px) * 4), t.get_mut(i)) {
                *slot = p;
                valid |= 1 << i;
            }
        }
    }
    (t, valid)
}

/// Quantises 8-bit RGB to RGB565 with rounding: `(v*31+127)/255`, `(v*63+127)/255`.
#[inline]
fn quantise565(rgb: [u8; 3]) -> u16 {
    let [r, g, b] = rgb.map(u32::from);
    let r5 = (r * 31 + 127) / 255;
    let g6 = (g * 63 + 127) / 255;
    let b5 = (b * 31 + 127) / 255;
    ((r5 << 11) | (g6 << 5) | b5) as u16
}

#[inline]
fn sq(a: u8, b: u8) -> u32 {
    let d = u32::from(a.abs_diff(b));
    d * d
}

/// Index of the palette entry (among the first `count`) with the smallest squared RGB distance; ties go to
/// the lowest index.
#[inline]
fn nearest_colour(pal: &[[u8; 4]; 4], count: usize, px: &[u8; 4]) -> u32 {
    let (mut best, mut best_d) = (0u32, u32::MAX);
    for (i, p) in pal.iter().take(count).enumerate() {
        let d = sq(p[0], px[0]) + sq(p[1], px[1]) + sq(p[2], px[2]);
        if d < best_d {
            best_d = d;
            best = i as u32;
        }
    }
    best
}

/// Range-fit colour block (8 bytes). `bc1`: texels with `A < 128` select the 3-colour mode and index 3.
fn encode_colour(t: &Texels, valid: u16, bc1: bool) -> [u8; 8] {
    let mut lo = [255u8; 3];
    let mut hi = [0u8; 3];
    let (mut any_opaque, mut any_transparent) = (false, false);
    for (i, px) in t.iter().enumerate() {
        if (valid >> i) & 1 == 0 {
            continue;
        }
        if bc1 && px[3] < 128 {
            any_transparent = true;
            continue;
        }
        any_opaque = true;
        lo = [lo[0].min(px[0]), lo[1].min(px[1]), lo[2].min(px[2])];
        hi = [hi[0].max(px[0]), hi[1].max(px[1]), hi[2].max(px[2])];
    }
    if !any_opaque {
        // Only reachable for a fully transparent BC1 block: c0 = c1 = 0 (3-colour mode), every index 3.
        return [0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
    }
    let three = bc1 && any_transparent;
    let (q_lo, q_hi) = (quantise565(lo), quantise565(hi));
    // 4-colour mode: c0 = max >= c1 = min. 3-colour mode (BC1 only) needs c0 <= c1, i.e. the swapped pair.
    let (c0, c1) = if three { (q_lo, q_hi) } else { (q_hi, q_lo) };
    let flat = !three && c0 == c1;
    let pal = colour_palette(c0, c1, !three);
    let entries = if three { 3 } else { 4 };
    let mut idx = 0u32;
    if !flat {
        for (i, px) in t.iter().enumerate() {
            if (valid >> i) & 1 == 0 {
                continue;
            }
            let v = if three && px[3] < 128 {
                3
            } else {
                nearest_colour(&pal, entries, px)
            };
            idx |= v << (2 * i);
        }
    }
    let [a, b] = c0.to_le_bytes();
    let [c, d] = c1.to_le_bytes();
    let [e, f, g, h] = idx.to_le_bytes();
    [a, b, c, d, e, f, g, h]
}

/// BC2 alpha block: `(a*15+127)/255` per texel, texel i in byte `i / 2`, low nibble first.
fn encode_bc2_alpha(t: &Texels, valid: u16) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    for (i, px) in t.iter().enumerate() {
        if (valid >> i) & 1 == 0 {
            continue;
        }
        let nibble = (u32::from(px[3]) * 15 + 127) / 255;
        if let Some(b) = bytes.get_mut(i / 2) {
            *b |= (nibble << (4 * (i & 1))) as u8;
        }
    }
    bytes
}

/// BC3 alpha block: `a0 = max`, `a1 = min`; equal gives all-zero indices, else the 8-value mode with the
/// nearest palette value (ties to the lowest index).
fn encode_bc3_alpha(t: &Texels, valid: u16) -> [u8; 8] {
    let (mut lo, mut hi) = (255u8, 0u8);
    for (i, px) in t.iter().enumerate() {
        if (valid >> i) & 1 == 1 {
            lo = lo.min(px[3]);
            hi = hi.max(px[3]);
        }
    }
    if lo >= hi {
        // Flat (or empty) block.
        let a = if valid != 0 { hi } else { 0 };
        return [a, a, 0, 0, 0, 0, 0, 0];
    }
    let pal = bc3_alpha_palette(hi, lo);
    let mut bits = 0u64;
    for (i, px) in t.iter().enumerate() {
        if (valid >> i) & 1 == 0 {
            continue;
        }
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

fn encode(kind: Kind, rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let Some(need) = w.checked_mul(h).and_then(|n| n.checked_mul(4)) else {
        return Vec::new();
    };
    if w == 0 || h == 0 || rgba.len() < need {
        return Vec::new();
    }
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = Vec::with_capacity(bw.saturating_mul(bh).saturating_mul(kind.block_bytes()));
    for by in 0..bh {
        for bx in 0..bw {
            let (t, valid) = gather(rgba, w, h, bx, by);
            match kind {
                Kind::Bc1 => {}
                Kind::Bc2 => out.extend_from_slice(&encode_bc2_alpha(&t, valid)),
                Kind::Bc3 => out.extend_from_slice(&encode_bc3_alpha(&t, valid)),
            }
            out.extend_from_slice(&encode_colour(&t, valid, kind == Kind::Bc1));
        }
    }
    out
}

/// Range-fit BC1 encoder (Plan/30 §11.6). `rgba` is `w * h * 4` bytes, top-down. Texels with `A < 128`
/// select the 3-colour mode with a transparent index. Returns `ceil(w/4) * ceil(h/4) * 8` bytes, or an empty
/// vector when a side is zero or `rgba` is shorter than `w * h * 4`. Deterministic and integer-only.
pub fn encode_bc1(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    encode(Kind::Bc1, rgba, w, h)
}

/// Range-fit BC2 encoder: 4-bit alpha `(a*15+127)/255` plus the opaque colour fit of [`encode_bc1`]. See
/// [`encode_bc1`] for the input rules.
pub fn encode_bc2(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    encode(Kind::Bc2, rgba, w, h)
}

/// Range-fit BC3 encoder: `a0 = max`, `a1 = min` alpha with nearest-index selection plus the colour fit of
/// [`encode_bc1`]. See [`encode_bc1`] for the input rules.
pub fn encode_bc3(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    encode(Kind::Bc3, rgba, w, h)
}
