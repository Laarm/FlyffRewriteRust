//! The DDS pack (`pack::dds::build`, Plan/30 §13.6): texture levels laid out for direct WebGL2 upload.
//!
//! [`BcMode::Passthrough`] keeps the BC1/2/3 blocks byte for byte and converts the uncompressed formats with
//! [`to_gl_upload`] (`A1R5G5B5`/`X1R5G5B5` become `UNSIGNED_SHORT_5_5_5_1`, `A8R8G8B8` is swapped to RGBA);
//! [`BcMode::Rgba8`] decodes every level to RGBA8 (the fallback when `WEBGL_compressed_texture_s3tc` is
//! missing). Only face 0 of a cube map is packed unless `all_faces` is set. Rows are tightly packed, so the
//! uploader sets `UNPACK_ALIGNMENT` to 1 for odd widths of the 2-byte formats.
//!
//! Level views: BC formats are `u8` (`comps 1`, `count` = bytes), `rgb565`/`rgba5551` are `u16` (`comps 1`,
//! `count` = texels), `rgba8` is `u8` with `comps 4` (`count` = texels).

use super::blob::{BlobBuilder, BlobView, ElemType, Packed};
use crate::FormatKind;
use crate::dds::{DdsFormat, DdsView, facts};
use crate::error::{FormatError, Parsed};
use crate::texture::{GlUpload, decode_level, to_gl_upload};

/// What to do with compressed levels (Plan/30 §13.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum BcMode {
    /// Keep BC1/2/3 blocks (the GPU supports S3TC) and convert uncompressed formats to their GL layout.
    #[default]
    Passthrough,
    /// Decode every level to RGBA8.
    Rgba8,
}

/// Options of [`build`] (`decode_dds` / `decode_dds_bc` `opts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(default))]
pub struct DdsPackOptions {
    /// Compressed-level handling (`decode_dds` forces `Passthrough`, `decode_dds_bc` forces `Rgba8`).
    pub bc: BcMode,
    /// Pack all six faces of a cube map instead of face 0 only.
    pub all_faces: bool,
}

/// The pixel format stored in the file (Plan/30 §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum SrcFormat {
    /// `DXT1`.
    Bc1,
    /// `DXT3`.
    Bc2,
    /// `DXT5`.
    Bc3,
    /// 16-bit `A1R5G5B5`.
    A1r5g5b5,
    /// 16-bit `X1R5G5B5`.
    X1r5g5b5,
    /// 16-bit `R5G6B5`.
    R5g6b5,
    /// 32-bit `A8R8G8B8`.
    A8r8g8b8,
    /// Any other RGB(A) mask layout (decoded to RGBA8).
    #[cfg_attr(feature = "wasm", serde(rename = "other"))]
    OtherRgb,
}

impl From<DdsFormat> for SrcFormat {
    fn from(f: DdsFormat) -> SrcFormat {
        match f {
            DdsFormat::Bc1 => SrcFormat::Bc1,
            DdsFormat::Bc2 => SrcFormat::Bc2,
            DdsFormat::Bc3 => SrcFormat::Bc3,
            DdsFormat::A1R5G5B5 => SrcFormat::A1r5g5b5,
            DdsFormat::X1R5G5B5 => SrcFormat::X1r5g5b5,
            DdsFormat::R5G6B5 => SrcFormat::R5g6b5,
            DdsFormat::A8R8G8B8 => SrcFormat::A8r8g8b8,
            DdsFormat::OtherRgb { .. } => SrcFormat::OtherRgb,
        }
    }
}

/// One packed mip level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LevelMeta {
    /// Cube face (0 for ordinary textures).
    pub face: u32,
    /// Level width in texels.
    pub w: u32,
    /// Level height in texels.
    pub h: u32,
    /// The level's bytes in the blob (see the module docs for the element types).
    pub view: BlobView,
}

/// The structured description of a DDS pack (Plan/30 §13.6).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DdsMeta {
    /// The format stored in the file.
    pub src_format: SrcFormat,
    /// The upload format of every packed level.
    pub pixel: GlUpload,
    /// Level 0 width.
    pub width: u32,
    /// Level 0 height.
    pub height: u32,
    /// Mip levels per face stored in the file.
    pub mip_count: u32,
    /// `true` when the file holds the full chain down to 1x1.
    pub full_chain: bool,
    /// `true` for the 3 corpus textures whose chain stops early.
    pub partial_chain: bool,
    /// `true` for cube maps (only face 0 is packed unless `all_faces`).
    pub cube: bool,
    /// `true` when a side is not a power of two.
    pub npot: bool,
    /// The packed levels: face-major, level 0 first.
    pub levels: Vec<LevelMeta>,
    /// Parser warnings, `"<WarningCode>: <detail>"`.
    pub warnings: Vec<String>,
}

/// Builds the pack of a parsed DDS (Plan/30 §13.6).
///
/// Errors: the `decode_level` / `to_gl_upload` errors (`UnexpectedEof` for a level shorter than its
/// dimensions, `DimensionTooLarge` above the decode caps) and `Inconsistent` for a blob above 4 GiB.
pub fn build(
    parsed: &Parsed<DdsView<'_>>,
    opts: &DdsPackOptions,
) -> Result<Packed<DdsMeta>, FormatError> {
    let view = &parsed.value;
    let f = facts(view);
    let faces = if opts.all_faces { view.face_count() } else { 1 };
    let mut bb = BlobBuilder::for_format(FormatKind::Dds);
    let mut levels = Vec::with_capacity(faces * view.level_count());
    let mut pixel: Option<GlUpload> = None;
    let mut scratch = Vec::new();
    for face in 0..faces {
        for level in 0..view.level_count() {
            let (w, h, _) = view
                .levels
                .get(face)
                .and_then(|f| f.get(level))
                .copied()
                .ok_or_else(|| {
                    FormatError::new(
                        FormatKind::Dds,
                        0,
                        crate::ErrorKind::InvalidValue {
                            field: "level",
                            value: level as i64,
                        },
                    )
                })?;
            let (kind, section) = match opts.bc {
                BcMode::Passthrough => {
                    let kind = to_gl_upload(view, face, level, &mut scratch)?;
                    (kind, scratch.as_slice())
                }
                BcMode::Rgba8 => {
                    let img = decode_level(view, face, level)?;
                    scratch = img.pixels;
                    (GlUpload::Rgba8, scratch.as_slice())
                }
            };
            let (ty, comps) = match kind {
                GlUpload::Bc1 | GlUpload::Bc2 | GlUpload::Bc3 => (ElemType::U8, 1),
                GlUpload::Rgb565 | GlUpload::Rgba5551 => (ElemType::U16, 1),
                GlUpload::Rgba8 => (ElemType::U8, 4),
            };
            pixel.get_or_insert(kind);
            levels.push(LevelMeta {
                face: face as u32,
                w,
                h,
                view: bb.push_typed(ty, comps, section)?,
            });
        }
    }
    let meta = DdsMeta {
        src_format: view.format.into(),
        pixel: pixel.unwrap_or(GlUpload::Rgba8),
        width: view.header.width,
        height: view.header.height,
        mip_count: f.levels,
        full_chain: f.full_chain,
        partial_chain: f.partial_chain,
        cube: f.cube,
        npot: f.npot,
        levels,
        warnings: parsed
            .warnings
            .iter()
            .map(|w| format!("{:?}: {}", w.code, w.detail))
            .collect(),
    };
    Ok(Packed {
        meta,
        blob: bb.finish(),
    })
}
