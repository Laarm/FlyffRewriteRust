//! BMP decoder (Plan/30 §11.5; ref/21 §6.10).
//!
//! `BITMAPFILEHEADER` (14 B) + `BITMAPINFOHEADER` (40 B or larger; only the first 40 bytes are read), an
//! optional palette (bpp <= 8) at `14 + header size`, pixel rows padded to 4 bytes starting at `bfOffBits`.
//! Supported: BI_RGB only; 1/4/8 bpp palettes, 24 bpp (BGR) and 32 bpp (BGRX); bottom-up (`height > 0`) and
//! top-down (`height < 0`) rows.
//!
//! * 32 bpp: **the fourth byte is ignored and alpha is 255** (the three 32-bpp corpus files store 0 there and
//!   D3DX loaded them as `X8R8G8B8`; Q19).
//! * The image-size field is ignored (445 of the 551 corpus files store a value that disagrees with the
//!   geometry); sizes come from width, height and bit depth.
//! * A palette index beyond the stored palette reads as opaque black.
//! * Up to 4 bytes after the pixels are normal (542 corpus files end with 2 padding bytes); more produce
//!   `ImageTrailingData`.
//!
//! The magenta colour key of the UI bitmaps is *not* applied here: use `texture::apply_color_key` with
//! `ColorKey::Exact(0xFFFF00FF)`.

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::image::check_decode_dims;
use crate::image::texture::Rgba8Image;
use crate::reader::ByteReader;

/// Largest trailing run (bytes) after the pixel data that is not reported.
pub const TRAILING_OK: usize = 4;

/// `BITMAPFILEHEADER` + the first 40 bytes of `BITMAPINFOHEADER`, as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BmpHeader {
    /// `bfSize`.
    pub file_size: u32,
    /// `bfOffBits`: offset of the first pixel row.
    pub pixel_offset: u32,
    /// `biSize` (40 or larger).
    pub header_size: u32,
    /// `biWidth`.
    pub width: i32,
    /// `biHeight`: positive = bottom-up, negative = top-down.
    pub height: i32,
    /// `biPlanes`.
    pub planes: u16,
    /// `biBitCount`.
    pub bits_per_pixel: u16,
    /// `biCompression` (0 = BI_RGB).
    pub compression: u32,
    /// `biSizeImage`.
    pub image_size: u32,
    /// `biXPelsPerMeter`.
    pub x_pels_per_meter: i32,
    /// `biYPelsPerMeter`.
    pub y_pels_per_meter: i32,
    /// `biClrUsed`.
    pub colours_used: u32,
    /// `biClrImportant`.
    pub colours_important: u32,
}

impl BmpHeader {
    /// `true` when the rows are stored bottom-up.
    pub fn bottom_up(&self) -> bool {
        self.height > 0
    }

    /// Number of palette entries stored for an indexed image (`biClrUsed` or `2^bpp`, at most `2^bpp`); 0 for
    /// true-colour images.
    pub fn palette_entries(&self) -> usize {
        if self.bits_per_pixel > 8 {
            return 0;
        }
        let max = 1usize << self.bits_per_pixel;
        match self.colours_used {
            0 => max,
            n => (n as usize).min(max),
        }
    }

    fn read(r: &mut ByteReader<'_>) -> Result<BmpHeader, FormatError> {
        let magic = r.u32()?.to_le_bytes();
        if !magic.starts_with(b"BM") {
            return Err(r.err_at(
                0,
                ErrorKind::BadMagic {
                    expected: "BM",
                    found: magic,
                },
            ));
        }
        // `r` is after the first four bytes; those include the low half of bfSize: re-read from 2.
        r.seek(2)?;
        let file_size = r.u32()?;
        r.skip(4)?; // two reserved words
        let pixel_offset = r.u32()?;
        let header_size = r.u32()?;
        if header_size < 40 {
            return Err(r.err_at(
                14,
                ErrorKind::InvalidValue {
                    field: "biSize",
                    value: i64::from(header_size),
                },
            ));
        }
        Ok(BmpHeader {
            file_size,
            pixel_offset,
            header_size,
            width: r.i32()?,
            height: r.i32()?,
            planes: r.u16()?,
            bits_per_pixel: r.u16()?,
            compression: r.u32()?,
            image_size: r.u32()?,
            x_pels_per_meter: r.i32()?,
            y_pels_per_meter: r.i32()?,
            colours_used: r.u32()?,
            colours_important: r.u32()?,
        })
    }
}

/// Reads and validates the headers (compression, bit depth, dimension caps, pixel offset); no pixels are
/// decoded.
pub fn header(b: &[u8]) -> Result<BmpHeader, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Bmp);
    let h = BmpHeader::read(&mut r)?;
    validate(&h)?;
    Ok(h)
}

fn validate(h: &BmpHeader) -> Result<(), FormatError> {
    if h.compression != 0 {
        return Err(FormatError::new(
            FormatKind::Bmp,
            30,
            ErrorKind::UnsupportedPixelFormat {
                detail: format!(
                    "BMP compression {} (only BI_RGB is supported)",
                    h.compression
                ),
            },
        ));
    }
    if !matches!(h.bits_per_pixel, 1 | 4 | 8 | 24 | 32) {
        return Err(FormatError::new(
            FormatKind::Bmp,
            28,
            ErrorKind::UnsupportedPixelFormat {
                detail: format!("{}-bit BMP (supported: 1, 4, 8, 24, 32)", h.bits_per_pixel),
            },
        ));
    }
    if h.width <= 0 {
        return Err(FormatError::new(
            FormatKind::Bmp,
            18,
            ErrorKind::InvalidValue {
                field: "biWidth",
                value: i64::from(h.width),
            },
        ));
    }
    if h.height == 0 {
        return Err(FormatError::new(
            FormatKind::Bmp,
            22,
            ErrorKind::InvalidValue {
                field: "biHeight",
                value: 0,
            },
        ));
    }
    check_decode_dims(
        FormatKind::Bmp,
        18,
        h.width.unsigned_abs(),
        h.height.unsigned_abs(),
    )?;
    if u64::from(h.pixel_offset) < 14 + u64::from(h.header_size) {
        return Err(FormatError::new(
            FormatKind::Bmp,
            10,
            ErrorKind::InvalidValue {
                field: "bfOffBits",
                value: i64::from(h.pixel_offset),
            },
        ));
    }
    Ok(())
}

/// Bytes per stored row: `((width * bpp + 31) / 32) * 4`.
pub(crate) fn row_stride(width: u32, bits_per_pixel: u32) -> usize {
    let bits = u64::from(width) * u64::from(bits_per_pixel);
    (bits.div_ceil(32) * 4) as usize
}

/// Reads `count` palette entries (`B, G, R, X` quads) at `offset`, as opaque RGBA. A palette that does not
/// fit in the file is an `UnexpectedEof`.
pub(crate) fn read_palette(
    b: &[u8],
    offset: usize,
    count: usize,
    kind: FormatKind,
) -> Result<Vec<[u8; 4]>, FormatError> {
    let mut r = ByteReader::new(b, kind);
    r.seek(offset)?;
    let quads = r.bytes(count * 4)?;
    Ok(quads
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[blue, green, red, _]| [red, green, blue, 255])
        .collect())
}

/// Looks up the palette index of pixel `x` in a packed row of `bpp` (1, 4 or 8) bits, MSB first.
#[inline]
pub(crate) fn index_at(row: &[u8], x: usize, bpp: usize) -> u8 {
    let bit = x * bpp;
    let byte = row.get(bit / 8).copied().unwrap_or(0);
    let shift = 8 - bpp - (bit % 8);
    (byte >> shift) & (((1u16 << bpp) - 1) as u8)
}

/// Decodes a BMP file to RGBA8 (rows top-down). Errors: `BadMagic`, `UnexpectedEof`, `InvalidValue` (header
/// size, zero or negative size, pixel offset inside the headers), `UnsupportedPixelFormat` (compression, bit
/// depths other than 1/4/8/24/32), `DimensionTooLarge`. Warning: `ImageTrailingData` (more than 4 bytes after the
/// pixels).
pub fn parse(b: &[u8]) -> Result<Parsed<Rgba8Image>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::Bmp);
    let h = BmpHeader::read(&mut r)?;
    validate(&h)?;
    let (w, ht) = (h.width.unsigned_abs(), h.height.unsigned_abs());
    let bpp = usize::from(h.bits_per_pixel);
    let stride = row_stride(w, u32::from(h.bits_per_pixel));

    // The palette sits right after the info header; it is clamped to the room before the pixel data.
    let palette_at = 14 + h.header_size as usize;
    let palette = if bpp <= 8 {
        let room = (h.pixel_offset as usize).saturating_sub(palette_at) / 4;
        read_palette(
            b,
            palette_at,
            h.palette_entries().min(room),
            FormatKind::Bmp,
        )?
    } else {
        Vec::new()
    };

    r.seek(h.pixel_offset as usize)?;
    let data = r.bytes(stride * ht as usize)?;
    let mut warnings = Vec::new();
    if r.remaining() > TRAILING_OK {
        warnings.push(FormatWarning::new(
            r.pos(),
            WarningCode::ImageTrailingData,
            format!("{} byte(s) after the pixel data", r.remaining()),
        ));
    }

    let out_stride = w as usize * 4;
    let mut pixels = vec![0u8; out_stride * ht as usize];
    let black = [0u8, 0, 0, 255];
    let src_rows = data.chunks_exact(stride);
    let dst_rows = pixels.chunks_exact_mut(out_stride);
    let convert = |dst: &mut [u8], src: &[u8]| match bpp {
        24 => {
            for (o, &[bl, g, rd]) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<3>().0)
            {
                o.copy_from_slice(&[rd, g, bl, 255]);
            }
        }
        32 => {
            for (o, &[bl, g, rd, _]) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                o.copy_from_slice(&[rd, g, bl, 255]);
            }
        }
        _ => {
            for (x, o) in dst.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let i = usize::from(index_at(src, x, bpp));
                o.copy_from_slice(palette.get(i).unwrap_or(&black));
            }
        }
    };
    if h.bottom_up() {
        for (dst, src) in dst_rows.rev().zip(src_rows) {
            convert(dst, src);
        }
    } else {
        for (dst, src) in dst_rows.zip(src_rows) {
            convert(dst, src);
        }
    }
    Ok(Parsed {
        value: Rgba8Image {
            width: w,
            height: ht,
            pixels,
        },
        warnings,
    })
}
