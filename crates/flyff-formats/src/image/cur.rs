//! `.cur` cursor decoder (Plan/30 §11.7; ref/22 §6.10).
//!
//! `ICONDIR` (6 B: reserved 0, type 2, count) + `count` x `ICONDIRENTRY` (16 B: width, height, colours,
//! reserved, hotspot x, hotspot y, byte size, offset) + per image a DIB: a `BITMAPINFOHEADER` whose height is
//! twice the image height, the palette (bpp <= 8), the XOR bitmap (bottom-up, rows padded to 4 bytes) and the
//! AND mask (1 bpp, rows padded to 4).
//!
//! Pixel rule: the XOR colour with alpha 0 where the AND bit is 1 and alpha 255 elsewhere. An AND = 1 pixel
//! with a non-black XOR colour ("invert screen") cannot be expressed: it becomes the opaque XOR colour and
//! the image gets one `CursorInvertPixel` warning (none in the corpus). A 32-bpp cursor with any non-zero
//! alpha is taken as ARGB (the AND mask is ignored), as Windows does. PNG-compressed images are
//! `UnsupportedPixelFormat`. Sizes come from the DIB header; the directory's byte-size field is not used.
//!
//! Corpus: 23 files, one 32x32 image each (21 x 24 bpp, 2 x 1 bpp), every file ends exactly at the end of its
//! image; hotspots are the directory values (`curBase.cur` (1, 1), `curCamera.cur` (16, 19)).

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::image::bmp::{index_at, read_palette, row_stride};
use crate::image::check_decode_dims;
use crate::image::texture::Rgba8Image;
use crate::reader::ByteReader;

/// The PNG signature that marks a PNG-compressed icon image.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// One decoded cursor image with its hotspot (Plan/30 §11.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorImage {
    /// Hotspot `(x, y)` in pixels from the top-left corner.
    pub hotspot: (u16, u16),
    /// The image (RGBA8, rows top-down).
    pub image: Rgba8Image,
}

/// One `ICONDIRENTRY`.
#[derive(Clone, Copy)]
struct Entry {
    hotspot: (u16, u16),
    offset: u32,
}

/// Decodes a `.cur` file. Errors: `InvalidValue` (reserved word, type other than 2, DIB header size, odd or
/// non-positive DIB height), `CountOutOfRange` (`idCount` of 0 or more entries than the file can hold),
/// `UnexpectedEof`, `UnsupportedPixelFormat` (PNG images, compression, bit depths other than 1/4/8/24/32),
/// `DimensionTooLarge`. Warning: `CursorInvertPixel`.
pub fn parse(b: &[u8]) -> Result<Parsed<Vec<CursorImage>>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Cur);
    let reserved = r.u16()?;
    let kind = r.u16()?;
    let count = usize::from(r.u16()?);
    if reserved != 0 {
        return Err(r.err_at(
            0,
            ErrorKind::InvalidValue {
                field: "idReserved",
                value: i64::from(reserved),
            },
        ));
    }
    if kind != 2 {
        return Err(r.err_at(
            2,
            ErrorKind::InvalidValue {
                field: "idType",
                value: i64::from(kind),
            },
        ));
    }
    let max_entries = r.remaining() / 16;
    if count == 0 || count > max_entries {
        return Err(r.err_at(
            4,
            ErrorKind::CountOutOfRange {
                field: "idCount",
                value: count as i64,
                max: max_entries as i64,
            },
        ));
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        r.skip(4)?; // width, height, colour count, reserved (the DIB header is authoritative)
        let hotspot = (r.u16()?, r.u16()?);
        r.skip(4)?; // byte size
        entries.push(Entry {
            hotspot,
            offset: r.u32()?,
        });
    }
    let mut warnings = Vec::new();
    let mut images = Vec::with_capacity(count);
    for e in entries {
        images.push(decode_image(b, e, &mut warnings)?);
    }
    Ok(Parsed {
        value: images,
        warnings,
    })
}

/// One image's `RGBA` XOR pixel (alpha = stored alpha for 32 bpp, else 255).
#[inline]
fn xor_pixel(row: &[u8], x: usize, bpp: usize, palette: &[[u8; 4]]) -> [u8; 4] {
    match bpp {
        24 => match row.get(x * 3..x * 3 + 3) {
            Some(&[b, g, r]) => [r, g, b, 255],
            _ => [0, 0, 0, 255],
        },
        32 => match row.get(x * 4..x * 4 + 4) {
            Some(&[b, g, r, a]) => [r, g, b, a],
            _ => [0, 0, 0, 0],
        },
        _ => palette
            .get(usize::from(index_at(row, x, bpp)))
            .copied()
            .unwrap_or([0, 0, 0, 255]),
    }
}

fn decode_image(
    b: &[u8],
    entry: Entry,
    warnings: &mut Vec<FormatWarning>,
) -> Result<CursorImage, FormatError> {
    let off = entry.offset as usize;
    let mut r = ByteReader::new(b, FormatKind::Cur);
    r.seek(off)?;
    if b.get(off..off.saturating_add(8)) == Some(&PNG_SIGNATURE[..]) {
        return Err(r.err(ErrorKind::UnsupportedPixelFormat {
            detail: "PNG-compressed cursor image".to_owned(),
        }));
    }
    let header_size = r.u32()?;
    if header_size < 40 {
        return Err(r.err_at(
            off,
            ErrorKind::InvalidValue {
                field: "biSize",
                value: i64::from(header_size),
            },
        ));
    }
    let width = r.i32()?;
    let height2 = r.i32()?;
    let _planes = r.u16()?;
    let bpp = usize::from(r.u16()?);
    let compression = r.u32()?;
    r.skip(12)?; // image size and pixels per metre
    let clr_used = r.u32()?;
    if compression != 0 {
        return Err(r.err_at(
            off + 16,
            ErrorKind::UnsupportedPixelFormat {
                detail: format!("cursor DIB compression {compression}"),
            },
        ));
    }
    if !matches!(bpp, 1 | 4 | 8 | 24 | 32) {
        return Err(r.err_at(
            off + 14,
            ErrorKind::UnsupportedPixelFormat {
                detail: format!("{bpp}-bit cursor DIB (supported: 1, 4, 8, 24, 32)"),
            },
        ));
    }
    if width <= 0 {
        return Err(r.err_at(
            off + 4,
            ErrorKind::InvalidValue {
                field: "biWidth",
                value: i64::from(width),
            },
        ));
    }
    if height2 <= 0 || height2 % 2 != 0 {
        return Err(r.err_at(
            off + 8,
            ErrorKind::InvalidValue {
                field: "biHeight",
                value: i64::from(height2),
            },
        ));
    }
    let (w, h) = (width.unsigned_abs(), height2.unsigned_abs() / 2);
    check_decode_dims(FormatKind::Cur, off + 4, w, h)?;

    let palette_at = off.saturating_add(header_size as usize);
    let (palette, palette_end) = if bpp <= 8 {
        let max = 1usize << bpp;
        let n = match clr_used {
            0 => max,
            c => (c as usize).min(max),
        };
        (
            read_palette(b, palette_at, n, FormatKind::Cur)?,
            palette_at.saturating_add(n * 4),
        )
    } else {
        (Vec::new(), palette_at)
    };

    let (w_us, h_us) = (w as usize, h as usize);
    let xor_stride = row_stride(w, bpp as u32);
    let and_stride = row_stride(w, 1);
    r.seek(palette_end)?;
    let xor = r.bytes(xor_stride * h_us)?;
    // The AND mask is mandatory except for 32-bpp (ARGB) cursors, which some writers store without it.
    let and_mask = match r.bytes(and_stride * h_us) {
        Ok(m) => Some(m),
        Err(e) if bpp != 32 => return Err(e),
        Err(_) => None,
    };

    let argb = bpp == 32 && xor.as_chunks::<4>().0.iter().any(|&[_, _, _, a]| a != 0);
    let mut pixels = vec![0u8; w_us * h_us * 4];
    let mut inverting = 0usize;
    for (y, out_row) in pixels.chunks_exact_mut(w_us * 4).enumerate() {
        // Rows are stored bottom-up.
        let src_y = h_us - 1 - y;
        let xor_row = xor
            .get(src_y * xor_stride..(src_y + 1) * xor_stride)
            .unwrap_or(&[]);
        let and_row = and_mask
            .and_then(|m| m.get(src_y * and_stride..(src_y + 1) * and_stride))
            .unwrap_or(&[]);
        for (x, out) in out_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut px = xor_pixel(xor_row, x, bpp, &palette);
            if !argb {
                let masked = and_row
                    .get(x / 8)
                    .is_some_and(|&byte| (byte >> (7 - (x % 8))) & 1 == 1);
                px[3] = if !masked {
                    255
                } else if px[0] | px[1] | px[2] == 0 {
                    0
                } else {
                    inverting += 1;
                    255
                };
            }
            out.copy_from_slice(&px);
        }
    }
    if inverting > 0 {
        warnings.push(FormatWarning::new(
            off,
            WarningCode::CursorInvertPixel,
            format!(
                "{inverting} pixel(s) with AND = 1 and a non-black XOR colour are drawn opaque"
            ),
        ));
    }
    Ok(CursorImage {
        hotspot: entry.hotspot,
        image: Rgba8Image {
            width: w,
            height: h,
            pixels,
        },
    })
}
