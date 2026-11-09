//! TGA decoder (Plan/30 §11.4; ref/22 §6.10).
//!
//! Supported: image type 2 (raw truecolour) and type 10 (RLE truecolour; a packet header's bit 7 marks a run,
//! `count = (header & 0x7F) + 1`, and packets may cross row boundaries), 24 bpp (BGR, opaque) and 32 bpp (BGRA
//! as stored); bottom-up and top-down rows (descriptor bit 5) and right-to-left rows (bit 4). Colour-mapped and
//! greyscale types are `UnsupportedPixelFormat`. Everything after the pixels (the optional extension area and
//! the 26-byte v2 footer) is ignored without a warning: footers are normal.
//!
//! Corpus (854 files): type 2 / 32 bpp 611, type 2 / 24 bpp 235, type 10 / 32 bpp 8 (`Theme/`); all bottom-up,
//! all with the footer, 531 non-power-of-two.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, Parsed};
use crate::image::check_decode_dims;
use crate::image::texture::Rgba8Image;
use crate::reader::ByteReader;

/// Image type: raw truecolour.
pub const TYPE_TRUECOLOUR: u8 = 2;
/// Image type: run-length encoded truecolour.
pub const TYPE_TRUECOLOUR_RLE: u8 = 10;
/// Descriptor bit 5: rows are stored top to bottom.
pub const DESC_TOP_TO_BOTTOM: u8 = 0x20;
/// Descriptor bit 4: pixels of a row are stored right to left.
pub const DESC_RIGHT_TO_LEFT: u8 = 0x10;

/// The 18-byte TGA header, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TgaHeader {
    /// Length of the image-ID field after the header.
    pub id_length: u8,
    /// 0 = no colour map, 1 = a colour map is present.
    pub colour_map_type: u8,
    /// 2 = truecolour, 10 = RLE truecolour (1, 3, 9, 11 are unsupported).
    pub image_type: u8,
    /// First colour-map entry.
    pub colour_map_first: u16,
    /// Number of colour-map entries.
    pub colour_map_length: u16,
    /// Bits per colour-map entry.
    pub colour_map_depth: u8,
    /// X origin.
    pub x_origin: u16,
    /// Y origin.
    pub y_origin: u16,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Bits per pixel (24 or 32).
    pub bits_per_pixel: u8,
    /// Image descriptor (bits 0-3 alpha bits, bit 4 right-to-left, bit 5 top-to-bottom).
    pub descriptor: u8,
}

impl TgaHeader {
    /// `true` when rows are stored top to bottom (descriptor bit 5).
    pub fn top_to_bottom(&self) -> bool {
        self.descriptor & DESC_TOP_TO_BOTTOM != 0
    }

    /// `true` when each row is stored right to left (descriptor bit 4).
    pub fn right_to_left(&self) -> bool {
        self.descriptor & DESC_RIGHT_TO_LEFT != 0
    }

    /// `true` for the RLE image type.
    pub fn is_rle(&self) -> bool {
        self.image_type == TYPE_TRUECOLOUR_RLE
    }

    fn read(r: &mut ByteReader<'_>) -> Result<TgaHeader, FormatError> {
        Ok(TgaHeader {
            id_length: r.u8()?,
            colour_map_type: r.u8()?,
            image_type: r.u8()?,
            colour_map_first: r.u16()?,
            colour_map_length: r.u16()?,
            colour_map_depth: r.u8()?,
            x_origin: r.u16()?,
            y_origin: r.u16()?,
            width: r.u16()?,
            height: r.u16()?,
            bits_per_pixel: r.u8()?,
            descriptor: r.u8()?,
        })
    }
}

/// Reads and validates only the header (type, bit depth and dimension caps); no pixels are decoded.
pub fn header(b: &[u8]) -> Result<TgaHeader, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Tga);
    let h = TgaHeader::read(&mut r)?;
    validate(&h)?;
    Ok(h)
}

fn unsupported(detail: String) -> ErrorKind {
    ErrorKind::UnsupportedPixelFormat { detail }
}

fn validate(h: &TgaHeader) -> Result<(), FormatError> {
    let err = |kind| FormatError::new(FormatKind::Tga, 2, kind);
    match h.image_type {
        TYPE_TRUECOLOUR | TYPE_TRUECOLOUR_RLE => {}
        1 | 9 => return Err(err(unsupported("colour-mapped TGA".to_owned()))),
        3 | 11 => return Err(err(unsupported("greyscale TGA".to_owned()))),
        t => return Err(err(unsupported(format!("TGA image type {t}")))),
    }
    if !matches!(h.bits_per_pixel, 24 | 32) {
        return Err(FormatError::new(
            FormatKind::Tga,
            16,
            unsupported(format!(
                "{}-bit truecolour TGA (only 24 and 32 are supported)",
                h.bits_per_pixel
            )),
        ));
    }
    if h.colour_map_type > 1 {
        return Err(FormatError::new(
            FormatKind::Tga,
            1,
            ErrorKind::InvalidValue {
                field: "colorMapType",
                value: i64::from(h.colour_map_type),
            },
        ));
    }
    check_decode_dims(FormatKind::Tga, 12, u32::from(h.width), u32::from(h.height))
}

/// Converts one stored pixel (`B, G, R` or `B, G, R, A`) to RGBA.
#[inline]
fn to_rgba(px: &[u8]) -> [u8; 4] {
    match *px {
        [b, g, r, a] => [r, g, b, a],
        [b, g, r] => [r, g, b, 255],
        _ => [0, 0, 0, 0],
    }
}

/// Swaps the rows of `pixels` (rows of `stride` bytes) top to bottom, in place.
fn flip_rows(pixels: &mut [u8], stride: usize) {
    if stride == 0 {
        return;
    }
    let rows = pixels.len() / stride;
    for y in 0..rows / 2 {
        // BOUNDS: (rows - 1 - y) * stride <= rows * stride <= pixels.len() for every y < rows / 2.
        let (top, bottom) = pixels.split_at_mut((rows - 1 - y) * stride);
        if let (Some(a), Some(b)) = (
            top.get_mut(y * stride..(y + 1) * stride),
            bottom.get_mut(..stride),
        ) {
            a.swap_with_slice(b);
        }
    }
}

/// Decodes a TGA file to RGBA8 (rows top-down). Errors: `UnexpectedEof` (header, pixels), `UnsupportedPixelFormat`
/// (types other than 2 and 10, bit depths other than 24/32), `InvalidValue` (zero size, bad colour-map type),
/// `DimensionTooLarge` (above 8192 per side or 16 Mi texels). The result never carries warnings today (data
/// after the pixels is ignored silently).
pub fn parse(b: &[u8]) -> Result<Parsed<Rgba8Image>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Tga);
    let h = TgaHeader::read(&mut r)?;
    validate(&h)?;
    let (w, ht) = (usize::from(h.width), usize::from(h.height));
    let total = w * ht;
    let bytes_pp = usize::from(h.bits_per_pixel / 8);

    r.skip(usize::from(h.id_length))?;
    if h.colour_map_type == 1 {
        let entry_bytes = usize::from(h.colour_map_depth).div_ceil(8);
        r.skip(usize::from(h.colour_map_length) * entry_bytes)?;
    }

    let mut pixels;
    if h.is_rle() {
        // Every packet yields at most 128 pixels and costs at least 1 + bytes_pp bytes: reject inputs that cannot
        // possibly hold the image before allocating it.
        let min_bytes = total.div_ceil(128) * (1 + bytes_pp);
        if r.remaining() < min_bytes {
            return Err(r.err(ErrorKind::UnexpectedEof {
                needed: min_bytes as u64,
                remaining: r.remaining() as u64,
            }));
        }
        pixels = vec![0u8; total * 4];
        let mut slots = pixels.as_chunks_mut::<4>().0.iter_mut();
        let mut written = 0usize;
        while written < total {
            let head = r.u8()?;
            // A packet that runs past the end of the image is cut at the end (the surplus is ignored).
            let n = (usize::from(head & 0x7F) + 1).min(total - written);
            if head & 0x80 != 0 {
                let rgba = to_rgba(r.bytes(bytes_pp)?);
                for slot in slots.by_ref().take(n) {
                    slot.copy_from_slice(&rgba);
                }
            } else {
                let data = r.bytes(n * bytes_pp)?;
                for (slot, px) in slots.by_ref().take(n).zip(data.chunks_exact(bytes_pp)) {
                    slot.copy_from_slice(&to_rgba(px));
                }
            }
            written += n;
        }
    } else {
        let data = r.bytes(total * bytes_pp)?;
        pixels = vec![0u8; total * 4];
        for (slot, px) in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(data.chunks_exact(bytes_pp))
        {
            slot.copy_from_slice(&to_rgba(px));
        }
    }

    if h.right_to_left() {
        for row in pixels.chunks_exact_mut(w * 4) {
            row.as_chunks_mut::<4>().0.reverse();
        }
    }
    if !h.top_to_bottom() {
        flip_rows(&mut pixels, w * 4);
    }
    Ok(Parsed::clean(Rgba8Image {
        width: u32::from(h.width),
        height: u32::from(h.height),
        pixels,
    }))
}
