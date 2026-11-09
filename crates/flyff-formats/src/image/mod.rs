//! Texture and image codecs (Plan/30 §11): the BC1/2/3 block codec ([`bc`]), the pixel-level helpers on top
//! of DDS ([`texture`]: [`texture::Rgba8Image`], `decode_level`, `to_gl_upload`, colour keys, mip completion),
//! and the TGA, BMP and cursor decoders ([`tga`], [`bmp`], [`cur`]).
//!
//! The module is private; the five submodules are re-exported at the crate root under their plan names
//! (`flyff_formats::{bc, bmp, cur, texture, tga}`), next to [`crate::dds`].

pub mod bc;
pub mod bmp;
pub mod cur;
pub mod texture;
pub mod tga;

use crate::FormatKind;
use crate::error::{ErrorKind, FormatError};

/// Largest accepted image side for every decoder (Plan/30 §11.8, `limits::MAX_DDS_DIM`).
pub const MAX_DECODE_DIM: u32 = 8192;
/// Largest accepted `width * height` for every decoder: 16 Mi texels (Plan/30 §11.8).
pub const MAX_DECODE_TEXELS: u64 = 16 * 1024 * 1024;

/// Checks the decode caps of Plan/30 §11.8: `1 <= w, h <= 8192` and `w * h <= 16 Mi`. A zero side is
/// `InvalidValue`; anything above the caps is `DimensionTooLarge`. Offsets are reported as `offset`.
pub(crate) fn check_decode_dims(
    format: FormatKind,
    offset: usize,
    width: u32,
    height: u32,
) -> Result<(), FormatError> {
    if width == 0 {
        return Err(FormatError::new(
            format,
            offset,
            ErrorKind::InvalidValue {
                field: "width",
                value: 0,
            },
        ));
    }
    if height == 0 {
        return Err(FormatError::new(
            format,
            offset,
            ErrorKind::InvalidValue {
                field: "height",
                value: 0,
            },
        ));
    }
    let texels = u64::from(width) * u64::from(height);
    if width > MAX_DECODE_DIM || height > MAX_DECODE_DIM || texels > MAX_DECODE_TEXELS {
        return Err(FormatError::new(
            format,
            offset,
            ErrorKind::DimensionTooLarge { width, height },
        ));
    }
    Ok(())
}

/// `width * height * 4` as `usize` once the caps passed (cannot overflow `u32`-sized inputs under the caps;
/// the checked arithmetic keeps 32-bit targets honest).
pub(crate) fn rgba_len(
    format: FormatKind,
    offset: usize,
    width: u32,
    height: u32,
) -> Result<usize, FormatError> {
    check_decode_dims(format, offset, width, height)?;
    usize::try_from(u64::from(width) * u64::from(height) * 4).map_err(|_| {
        FormatError::new(
            format,
            offset,
            ErrorKind::DimensionTooLarge { width, height },
        )
    })
}

/// Reads `N` bytes at `offset` as an array (`None` when out of range).
#[inline]
pub(crate) fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    let end = offset.checked_add(N)?;
    <[u8; N]>::try_from(bytes.get(offset..end)?).ok()
}
