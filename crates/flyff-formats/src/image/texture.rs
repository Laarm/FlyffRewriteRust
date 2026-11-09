//! Pixel-level texture helpers on top of [`crate::dds`] (Plan/30 §11.3 and §11.6).
//!
//! Part 1: [`Rgba8Image`], [`decode_level`] (any DDS level to RGBA8) and [`to_gl_upload`] (the byte layout
//! WebGL2 wants for a level). Part 2: [`ColorKey`] / [`apply_color_key`], [`downsample_box`] and
//! [`complete_mips`] (the DDS fix for textures without or with an incomplete mip chain, decision D6).
//!
//! Everything is integer-only, so the output is bit-identical natively and under wasm.

use crate::FormatKind;
use crate::dds::{
    DATA_OFFSET, DDSCAPS_COMPLEX, DDSCAPS_MIPMAP, DDSD_MIPMAPCOUNT, DdsFormat, DdsView,
};
use crate::error::{ErrorKind, FormatError};
pub use crate::image::{MAX_DECODE_DIM, MAX_DECODE_TEXELS};
use crate::image::{array_at, bc, rgba_len};
use crate::limits::max_mip_levels;

/// Largest side `complete_mips` accepts (Plan/30 §11.8).
pub const MAX_MIPS_INPUT_DIM: u32 = 4096;

/// A decoded image: RGBA8, rows top-down, `width * height * 4` bytes (Plan/30 §11.3).
#[derive(Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `R, G, B, A` bytes, row by row from the top.
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Rgba8Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rgba8Image {{ {}x{}, {} bytes }}",
            self.width,
            self.height,
            self.pixels.len()
        )
    }
}

impl Rgba8Image {
    /// Wraps `pixels`; `InvalidValue` when its length is not `width * height * 4`.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Rgba8Image, FormatError> {
        let expected = u64::from(width) * u64::from(height) * 4;
        if pixels.len() as u64 != expected {
            return Err(FormatError::new(
                FormatKind::Dds,
                0,
                ErrorKind::InvalidValue {
                    field: "pixel buffer length",
                    value: pixels.len() as i64,
                },
            ));
        }
        Ok(Rgba8Image {
            width,
            height,
            pixels,
        })
    }

    /// The pixel at `(x, y)` (`None` outside the image).
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = (y as usize)
            .checked_mul(self.width as usize)?
            .checked_add(x as usize)?
            .checked_mul(4)?;
        array_at::<4>(&self.pixels, offset)
    }
}

// ---------------------------------------------------------------------------------------------------------
// Uncompressed channel conversion
// ---------------------------------------------------------------------------------------------------------

/// One colour channel of a bit-mask pixel format.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    bits: u32,
}

impl Channel {
    fn new(mask: u32) -> Channel {
        if mask == 0 {
            return Channel {
                mask: 0,
                shift: 0,
                bits: 0,
            };
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).trailing_ones();
        Channel { mask, shift, bits }
    }

    /// The channel of `px` scaled to 8 bits by bit replication (5 bits: `(v << 3) | (v >> 2)`).
    fn extract8(self, px: u32) -> u8 {
        if self.bits == 0 {
            return 0;
        }
        expand_bits((px & self.mask) >> self.shift, self.bits)
    }

    /// `v8` quantised into the channel with rounding, `(v * max + 127) / 255`, shifted into place.
    fn pack(self, v8: u8) -> u32 {
        if self.bits == 0 {
            return 0;
        }
        let max = u64::from(self.mask >> self.shift);
        let q = (u64::from(v8) * max + 127) / 255;
        ((q as u32) << self.shift) & self.mask
    }
}

/// Scales a `bits`-wide value to 8 bits by repeating its bit pattern (more than 8 bits: truncation).
fn expand_bits(v: u32, bits: u32) -> u8 {
    match bits {
        0 => 0,
        8 => v as u8,
        b if b > 8 => (v >> (b - 8)) as u8,
        b => {
            let mut acc = v << (8 - b);
            let mut have = b;
            while have < 8 {
                acc |= acc >> have;
                have *= 2;
            }
            acc as u8
        }
    }
}

/// The channel masks `R, G, B, A` of an uncompressed format (`None` for BC formats).
fn format_masks(format: DdsFormat) -> Option<[u32; 4]> {
    match format {
        DdsFormat::A1R5G5B5 => Some([0x7C00, 0x03E0, 0x001F, 0x8000]),
        DdsFormat::X1R5G5B5 => Some([0x7C00, 0x03E0, 0x001F, 0]),
        DdsFormat::R5G6B5 => Some([0xF800, 0x07E0, 0x001F, 0]),
        DdsFormat::A8R8G8B8 => Some([0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]),
        DdsFormat::OtherRgb { masks, .. } => Some(masks),
        _ => None,
    }
}

/// Converts tightly packed uncompressed pixels to RGBA8. `out` has `4 * pixel count` bytes.
fn convert_to_rgba(format: DdsFormat, data: &[u8], out: &mut [u8]) {
    match format {
        DdsFormat::R5G6B5 => {
            for (&[lo, hi], o) in data
                .as_chunks::<2>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                let v = u32::from(u16::from_le_bytes([lo, hi]));
                let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
                o.copy_from_slice(&[
                    ((r << 3) | (r >> 2)) as u8,
                    ((g << 2) | (g >> 4)) as u8,
                    ((b << 3) | (b >> 2)) as u8,
                    255,
                ]);
            }
        }
        DdsFormat::A1R5G5B5 | DdsFormat::X1R5G5B5 => {
            let has_alpha = format == DdsFormat::A1R5G5B5;
            for (&[lo, hi], o) in data
                .as_chunks::<2>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                let v = u32::from(u16::from_le_bytes([lo, hi]));
                let (r, g, b) = ((v >> 10) & 31, (v >> 5) & 31, v & 31);
                let a = if !has_alpha || v & 0x8000 != 0 {
                    255
                } else {
                    0
                };
                o.copy_from_slice(&[
                    ((r << 3) | (r >> 2)) as u8,
                    ((g << 3) | (g >> 2)) as u8,
                    ((b << 3) | (b >> 2)) as u8,
                    a,
                ]);
            }
        }
        DdsFormat::A8R8G8B8 => {
            for (&[b, g, r, a], o) in data
                .as_chunks::<4>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                o.copy_from_slice(&[r, g, b, a]);
            }
        }
        DdsFormat::OtherRgb { bits, masks } => {
            let bpp = (bits / 8) as usize;
            let [r, g, b, a] = masks.map(Channel::new);
            let has_alpha = a.bits != 0;
            if bpp == 0 {
                return;
            }
            for (px, o) in data
                .chunks_exact(bpp)
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                let v = px
                    .iter()
                    .enumerate()
                    .fold(0u32, |acc, (i, &byte)| acc | (u32::from(byte) << (8 * i)));
                let alpha = if has_alpha { a.extract8(v) } else { 255 };
                o.copy_from_slice(&[r.extract8(v), g.extract8(v), b.extract8(v), alpha]);
            }
        }
        DdsFormat::Bc1 | DdsFormat::Bc2 | DdsFormat::Bc3 => {}
    }
}

/// Encodes an RGBA image into the byte layout of `format` (BC via the range-fit encoder, uncompressed via
/// rounded channel quantisation: 5-bit `(v*31+127)/255`, 6-bit `(v*63+127)/255`, 1-bit alpha `a >= 128`).
fn encode_level(format: DdsFormat, img: &Rgba8Image) -> Vec<u8> {
    match format {
        DdsFormat::Bc1 => bc::encode_bc1(&img.pixels, img.width, img.height),
        DdsFormat::Bc2 => bc::encode_bc2(&img.pixels, img.width, img.height),
        DdsFormat::Bc3 => bc::encode_bc3(&img.pixels, img.width, img.height),
        _ => {
            let (Some(masks), Some(bpp)) = (format_masks(format), format.bytes_per_pixel()) else {
                return Vec::new();
            };
            let [r, g, b, a] = masks.map(Channel::new);
            let mut out = Vec::with_capacity(img.pixels.len() / 4 * bpp);
            for &[pr, pg, pb, pa] in img.pixels.as_chunks::<4>().0 {
                let v = r.pack(pr) | g.pack(pg) | b.pack(pb) | a.pack(pa);
                out.extend(v.to_le_bytes().iter().take(bpp));
            }
            out
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Part 1: decode_level, to_gl_upload
// ---------------------------------------------------------------------------------------------------------

/// Looks up `levels[face][level]` with a descriptive error.
fn level_of<'a>(
    d: &DdsView<'a>,
    face: usize,
    level: usize,
) -> Result<(u32, u32, &'a [u8]), FormatError> {
    let invalid = |field: &'static str, value: usize| {
        FormatError::new(
            FormatKind::Dds,
            DATA_OFFSET,
            ErrorKind::InvalidValue {
                field,
                value: value as i64,
            },
        )
    };
    let faces = d.levels.get(face).ok_or_else(|| invalid("face", face))?;
    faces
        .get(level)
        .copied()
        .ok_or_else(|| invalid("level", level))
}

/// Decodes one mip level of one face to RGBA8 (Plan/30 §11.3): BC1/2/3 through [`bc`], uncompressed formats by
/// channel expansion (`A1R5G5B5` alpha = bit 15, `X1R5G5B5` and `R5G6B5` opaque, `A8R8G8B8` swapped to RGBA).
///
/// Errors: `InvalidValue` for a bad `face`/`level`, `DimensionTooLarge` above 8192 per side or 16 Mi texels,
/// `UnexpectedEof` when the level data is shorter than its dimensions need.
pub fn decode_level(d: &DdsView<'_>, face: usize, level: usize) -> Result<Rgba8Image, FormatError> {
    let (w, h, data) = level_of(d, face, level)?;
    let len = rgba_len(FormatKind::Dds, DATA_OFFSET, w, h)?;
    let mut pixels = vec![0u8; len];
    match d.format {
        DdsFormat::Bc1 => bc::decode_bc1(data, w, h, &mut pixels)?,
        DdsFormat::Bc2 => bc::decode_bc2(data, w, h, &mut pixels)?,
        DdsFormat::Bc3 => bc::decode_bc3(data, w, h, &mut pixels)?,
        other => {
            let need = other.level_size(w, h) as usize;
            let Some(src) = data.get(..need) else {
                return Err(FormatError::new(
                    FormatKind::Dds,
                    DATA_OFFSET,
                    ErrorKind::UnexpectedEof {
                        needed: need as u64,
                        remaining: data.len() as u64,
                    },
                ));
            };
            convert_to_rgba(other, src, &mut pixels);
        }
    }
    Ok(Rgba8Image {
        width: w,
        height: h,
        pixels,
    })
}

/// How a level is uploaded to WebGL2 (Plan/30 §11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum GlUpload {
    /// `COMPRESSED_RGBA_S3TC_DXT1_EXT` blocks (8 B per 4x4).
    Bc1,
    /// `COMPRESSED_RGBA_S3TC_DXT3_EXT` blocks (16 B).
    Bc2,
    /// `COMPRESSED_RGBA_S3TC_DXT5_EXT` blocks (16 B).
    Bc3,
    /// `RGB` / `UNSIGNED_SHORT_5_6_5`, 2 bytes per texel.
    Rgb565,
    /// `RGBA` / `UNSIGNED_SHORT_5_5_5_1`, 2 bytes per texel.
    Rgba5551,
    /// `RGBA` / `UNSIGNED_BYTE`, 4 bytes per texel.
    Rgba8,
}

/// Fills `out` (cleared first) with the bytes to hand to WebGL2 for one level and says how (Plan/30 §11.3):
/// BC blocks are copied unchanged; `R5G6B5` is unchanged; `A1R5G5B5` becomes `((v & 0x7FFF) << 1) | (v >> 15)`
/// and `X1R5G5B5` `((v & 0x7FFF) << 1) | 1` (both little-endian `u16`, `UNSIGNED_SHORT_5_5_5_1`);
/// `A8R8G8B8` is swapped to RGBA; any other layout is decoded to RGBA8. Rows are tightly packed, so the
/// uploader must set `UNPACK_ALIGNMENT` to 1 for odd widths of the 2-byte formats.
pub fn to_gl_upload(
    d: &DdsView<'_>,
    face: usize,
    level: usize,
    out: &mut Vec<u8>,
) -> Result<GlUpload, FormatError> {
    let (w, h, data) = level_of(d, face, level)?;
    out.clear();
    let short = |need: usize| {
        FormatError::new(
            FormatKind::Dds,
            DATA_OFFSET,
            ErrorKind::UnexpectedEof {
                needed: need as u64,
                remaining: data.len() as u64,
            },
        )
    };
    let need = usize::try_from(d.format.level_size(w, h)).map_err(|_| {
        FormatError::new(
            FormatKind::Dds,
            DATA_OFFSET,
            ErrorKind::DimensionTooLarge {
                width: w,
                height: h,
            },
        )
    })?;
    match d.format {
        DdsFormat::Bc1 | DdsFormat::Bc2 | DdsFormat::Bc3 | DdsFormat::R5G6B5 => {
            out.extend_from_slice(data.get(..need).ok_or_else(|| short(need))?);
            Ok(match d.format {
                DdsFormat::Bc1 => GlUpload::Bc1,
                DdsFormat::Bc2 => GlUpload::Bc2,
                DdsFormat::Bc3 => GlUpload::Bc3,
                _ => GlUpload::Rgb565,
            })
        }
        DdsFormat::A1R5G5B5 | DdsFormat::X1R5G5B5 => {
            let src = data.get(..need).ok_or_else(|| short(need))?;
            let force_alpha = d.format == DdsFormat::X1R5G5B5;
            out.reserve(need);
            for &[lo, hi] in src.as_chunks::<2>().0 {
                let v = u16::from_le_bytes([lo, hi]);
                let alpha = if force_alpha { 1 } else { v >> 15 };
                out.extend_from_slice(&(((v & 0x7FFF) << 1) | alpha).to_le_bytes());
            }
            Ok(GlUpload::Rgba5551)
        }
        DdsFormat::A8R8G8B8 => {
            let src = data.get(..need).ok_or_else(|| short(need))?;
            out.reserve(need);
            for &[b, g, r, a] in src.as_chunks::<4>().0 {
                out.extend_from_slice(&[r, g, b, a]);
            }
            Ok(GlUpload::Rgba8)
        }
        DdsFormat::OtherRgb { .. } => {
            out.extend_from_slice(&decode_level(d, face, level)?.pixels);
            Ok(GlUpload::Rgba8)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Part 2: colour keys, box filter, mip completion
// ---------------------------------------------------------------------------------------------------------

/// A colour key: which pixels become transparent (Plan/30 §11.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorKey {
    /// Leave the image alone.
    None,
    /// D3DX colour key: a pixel whose `(A, R, G, B)` equals the key (`0xAARRGGBB`; opaque sources have alpha
    /// 255) becomes `(0, 0, 0, 0)`. UI DDS and BMP use `0xFFFF00FF` (ref/21 §6.10); texture packs pass their
    /// own per-file key (ref/22 §6.9).
    Exact(u32),
    /// `PaintTexture` rule for 24-bit skin tiles: pixels whose top nibbles are `(F, 0, F)` become transparent
    /// (ref/22 §6.4).
    NibbleMagenta,
    /// `PaintTexture` rule for 32-bit skin tiles: pixels with alpha below the threshold become transparent
    /// (the original uses 16).
    AlphaBelow(u8),
}

/// Applies a colour key in place. Cleared pixels become `(0, 0, 0, 0)`.
pub fn apply_color_key(img: &mut Rgba8Image, key: ColorKey) {
    match key {
        ColorKey::None => {}
        ColorKey::Exact(argb) => {
            let [a, r, g, b] = argb.to_be_bytes();
            for px in img.pixels.as_chunks_mut::<4>().0.iter_mut() {
                if *px == [r, g, b, a] {
                    px.fill(0);
                }
            }
        }
        ColorKey::NibbleMagenta => {
            for px in img.pixels.as_chunks_mut::<4>().0.iter_mut() {
                if let [r, g, b, _] = *px
                    && r >> 4 == 0xF
                    && g >> 4 == 0
                    && b >> 4 == 0xF
                {
                    px.fill(0);
                }
            }
        }
        ColorKey::AlphaBelow(threshold) => {
            for px in img.pixels.as_chunks_mut::<4>().0.iter_mut() {
                if let [_, _, _, a] = *px
                    && a < threshold
                {
                    px.fill(0);
                }
            }
        }
    }
}

/// Halves an image with a box filter (Plan/30 §11.6): output size `max(1, w/2) x max(1, h/2)`; every output
/// pixel is the mean per channel of its 2x2 source footprint, `(sum + n/2) / n` with `n` the number of
/// in-bounds source pixels (4, or 2 where a side is 1). The last source row/column of an odd side is dropped,
/// like the mip chain sizes `max(1, dim >> k)` imply.
pub fn downsample_box(img: &Rgba8Image) -> Rgba8Image {
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 {
        return img.clone();
    }
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut pixels = vec![0u8; nw * nh * 4];
    for (y, row) in pixels.chunks_exact_mut(nw * 4).enumerate() {
        for (x, out) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let (sx, sy) = (2 * x + dx, 2 * y + dy);
                    if sx >= w || sy >= h {
                        continue;
                    }
                    if let Some(p) = array_at::<4>(&img.pixels, (sy * w + sx) * 4) {
                        for (s, v) in sum.iter_mut().zip(p) {
                            *s += u32::from(v);
                        }
                        n += 1;
                    }
                }
            }
            for (o, s) in out.iter_mut().zip(sum) {
                *o = (s + n / 2).checked_div(n).unwrap_or(0) as u8;
            }
        }
    }
    Rgba8Image {
        width: nw as u32,
        height: nh as u32,
        pixels,
    }
}

/// Builds a new DDS file with a complete mip chain (the D6 fix; Plan/30 §11.6).
///
/// Every existing level is kept byte for byte. For each face the last existing level is decoded to RGBA,
/// halved with [`downsample_box`] down to 1x1 and every new level is re-encoded in the source format (BC via
/// the range-fit encoder, 16/32-bit with rounded quantisation). The header keeps all stored fields except
/// `MIPMAPCOUNT` (set), `dwMipMapCount` (the full chain length) and `DDSCAPS_COMPLEX | DDSCAPS_MIPMAP`
/// (set). A file that is already complete comes back unchanged (apart from those header bits). Cube maps are
/// completed per face.
///
/// Errors: `DimensionTooLarge` above 4096 per side, `InvalidValue` for a view without levels, plus the
/// errors of [`decode_level`].
pub fn complete_mips(d: &DdsView<'_>) -> Result<Vec<u8>, FormatError> {
    let (w, h) = (d.header.width, d.header.height);
    if w > MAX_MIPS_INPUT_DIM || h > MAX_MIPS_INPUT_DIM {
        return Err(FormatError::new(
            FormatKind::Dds,
            12,
            ErrorKind::DimensionTooLarge {
                width: w,
                height: h,
            },
        ));
    }
    let total = max_mip_levels(w, h);
    let mut header = d.header;
    header.flags |= DDSD_MIPMAPCOUNT;
    header.mip_map_count = total;
    header.caps |= DDSCAPS_COMPLEX | DDSCAPS_MIPMAP;

    let mut out = Vec::new();
    header.write_to(&mut out);
    for (face_index, face) in d.levels.iter().enumerate() {
        if face.is_empty() {
            return Err(FormatError::new(
                FormatKind::Dds,
                DATA_OFFSET,
                ErrorKind::InvalidValue {
                    field: "level count",
                    value: 0,
                },
            ));
        }
        for &(_, _, data) in face {
            out.extend_from_slice(data);
        }
        let have = face.len();
        if have >= total as usize {
            continue;
        }
        let mut img = decode_level(d, face_index, have - 1)?;
        for _ in have..total as usize {
            img = downsample_box(&img);
            out.extend_from_slice(&encode_level(d.format, &img));
        }
    }
    Ok(out)
}
