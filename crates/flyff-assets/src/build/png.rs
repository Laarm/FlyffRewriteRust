//! PNG conversion (`png` kind, Plan/24 §10.2, WP-WEB-08): TGA, BMP and DDS images of the UI and 3D
//! texture directories become PNGs the browser draws directly.
//!
//! Sources are decoded to RGBA8 by `flyff-formats` and then keyed according to the [`PngMode`]:
//!
//! | mode | rule per pixel (after decoding) | default for |
//! |---|---|---|
//! | [`PngMode::Exact`] | `(r, g, b) == (255, 0, 255)` and `a == 255` becomes `(0, 0, 0, 0)` (a source without an alpha channel has `a = 255`) | `theme`, `icon`, `item`, `char` |
//! | [`PngMode::Nibble`] | sources without alpha: top nibbles `(F, 0, F)` become `(0, 0, 0, 0)`, the rest `a = 255`; sources with alpha: `a < 16` becomes `(0, 0, 0, 0)`, the rest unchanged | the `.nk.png` key (doc 42's skin baker) |
//! | [`PngMode::None`] | unchanged | `model/texture`, `sfx`, `sfx/texture`, `weather` |
//!
//! The encoding is 8-bit RGBA (colour type 6), or RGB (type 2) when every alpha is 255, with the `png`
//! crate's default compression and adaptive filter and no ancillary chunks, so the bytes are
//! deterministic. PNG is already compressed: `Compress::Never`.
//!
//! **Fidelity (PL-139).** The original quantised BMP/DDS UI images to A1R5G5B5; the PNGs keep the full 8
//! bits per channel of the source.
//!
//! Keys: `<vfs path>.png` (the directory's default mode) and `<vfs path>.nk.png` (nibble key, only for the
//! directories whose default is `Exact`). The job name is the source path, with `.nk` appended for the
//! nibble variant.

use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{AssetBuilder, AssetKind, BuildCtx, BuiltFile, JobKey, KeyRoute};
use crate::res::{self, Form};
use flyff_formats::dds::{self, DdsFormat};
use flyff_formats::texture::{self, ColorKey, Rgba8Image};
use flyff_formats::{bmp, tga};

/// Bump on any change that alters the produced bytes (the colour keys, the encoder settings, a `png`
/// crate upgrade that changes its output).
pub const BUILDER_VERSION: u32 = 1;

/// The source directories (= partitions), in prewarm order: P2 `theme`, P4 `icon item char`, P6 the 3D
/// texture directories (§7.5).
pub const PNG_DIRS: [&str; 8] = [
    "theme",
    "icon",
    "item",
    "char",
    "model/texture",
    "sfx",
    "sfx/texture",
    "weather",
];

/// The colour-key mode of one conversion (§10.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PngMode {
    /// Exact magenta `FF00FF` keyed (`theme`, `icon`, `item`, `char`).
    Exact,
    /// The `PaintTexture` skin rule (`.nk.png` keys).
    Nibble,
    /// Unchanged (3D textures).
    None,
}

/// The default mode of a source directory (`None` for a directory without PNG conversion).
pub fn default_mode(dir: &str) -> Option<PngMode> {
    let form = [
        res::rule_for(dir, "tga"),
        res::rule_for(dir, "bmp"),
        res::rule_for(dir, "dds"),
    ]
    .into_iter()
    .flatten()
    .map(|r| r.form)
    .find(|f| matches!(f, Form::PngExact | Form::PngNone))?;
    Some(match form {
        Form::PngExact => PngMode::Exact,
        _ => PngMode::None,
    })
}

/// `true` for the image extensions the converter reads.
fn is_source_ext(ext: &str) -> bool {
    matches!(ext, "tga" | "bmp" | "dds")
}

/// `true` when the file `path` is converted to PNG: its directory and extension form a PNG rule.
pub fn is_png_source(path: &str) -> bool {
    res::ext_of(path).is_some_and(|ext| {
        is_source_ext(ext)
            && res::rule_for(res::dir_of(path), ext)
                .is_some_and(|r| matches!(r.form, Form::PngExact | Form::PngNone))
    })
}

/// Does a DDS pixel format carry an alpha channel (BC1 counts: it has 1-bit alpha)?
fn dds_has_alpha(f: DdsFormat) -> bool {
    match f {
        DdsFormat::Bc1
        | DdsFormat::Bc2
        | DdsFormat::Bc3
        | DdsFormat::A1R5G5B5
        | DdsFormat::A8R8G8B8 => true,
        DdsFormat::X1R5G5B5 | DdsFormat::R5G6B5 => false,
        DdsFormat::OtherRgb { masks, .. } => masks[3] != 0,
    }
}

/// Decodes a `tga`, `bmp` or `dds` file to RGBA8 and says whether the source has an alpha channel (a TGA
/// with 32 bits per pixel, a DDS format with alpha; BMPs never have one: the decoder sets `a = 255`).
pub fn decode_source(ext: &str, bytes: &[u8]) -> Result<(Rgba8Image, bool), String> {
    match ext {
        "tga" => {
            let alpha = tga::header(bytes)
                .map_err(|e| e.to_string())?
                .bits_per_pixel
                == 32;
            let img = tga::parse(bytes).map_err(|e| e.to_string())?.value;
            Ok((img, alpha))
        }
        "bmp" => Ok((bmp::parse(bytes).map_err(|e| e.to_string())?.value, false)),
        "dds" => {
            let parsed = dds::view(bytes).map_err(|e| e.to_string())?;
            let view = parsed.value;
            let img = texture::decode_level(&view, 0, 0).map_err(|e| e.to_string())?;
            Ok((img, dds_has_alpha(view.format)))
        }
        other => Err(format!("unsupported image extension {other:?}")),
    }
}

/// Applies the colour key of `mode` in place.
pub fn apply_mode(img: &mut Rgba8Image, has_alpha: bool, mode: PngMode) {
    match mode {
        PngMode::None => {}
        PngMode::Exact => texture::apply_color_key(img, ColorKey::Exact(0xFFFF_00FF)),
        PngMode::Nibble => {
            // Sources without alpha: the decoders deliver `a = 255` for every pixel, so keying the
            // (F, 0, F) nibbles is the whole rule (a keyed pixel becomes (0, 0, 0, 0)).
            let key = if has_alpha {
                ColorKey::AlphaBelow(16)
            } else {
                ColorKey::NibbleMagenta
            };
            texture::apply_color_key(img, key);
        }
    }
}

/// Encodes RGBA8 pixels as a PNG: RGB (type 2) when every alpha is 255, else RGBA (type 6), 8 bits.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let opaque = rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 255);
    let mut out = Vec::with_capacity(rgba.len() / 2 + 64);
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::default());
        enc.set_filter(png::Filter::default());
        let w = if opaque {
            enc.set_color(png::ColorType::Rgb);
            let rgb: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, _]| [r, g, b])
                .collect();
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(&rgb).map_err(|e| e.to_string())?;
            w
        } else {
            enc.set_color(png::ColorType::Rgba);
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(rgba).map_err(|e| e.to_string())?;
            w
        };
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Converts the bytes of a source file to a PNG in `mode`.
pub fn convert(ext: &str, bytes: &[u8], mode: PngMode) -> Result<Vec<u8>, String> {
    let (mut img, has_alpha) = decode_source(ext, bytes)?;
    apply_mode(&mut img, has_alpha, mode);
    encode_png(img.width, img.height, &img.pixels)
}

/// The builder of kind [`AssetKind::UiPng`]: one job per source file and mode; partition = source
/// directory.
#[derive(Clone, Copy, Debug, Default)]
pub struct PngBuilder;

/// The suffix of the nibble-mode key and job name.
const NK: &str = ".nk";

impl AssetBuilder for PngBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::UiPng
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    /// `<vfs path>.png` or `<vfs path>.nk.png`.
    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let Some(stem) = key.strip_suffix(".png") else {
            return Err(AssetError::bad_key(format!(
                "{key:?}: a png key ends in .png"
            )));
        };
        // A source never ends in `.nk` (its extension is tga, bmp or dds), so the suffix is unambiguous.
        let (path, nibble) = match stem.strip_suffix(NK) {
            Some(p) => (p, true),
            None => (stem, false),
        };
        let entry = ctx
            .vfs
            .resolve(path)
            .ok_or_else(|| AssetError::not_found(format!("no such file: {path:?}")))?;
        if entry.path.as_str() != path {
            return Err(AssetError::bad_key(format!(
                "not a canonical path: {path:?}"
            )));
        }
        let dir = res::dir_of(path);
        if !is_png_source(path) {
            return Err(AssetError::not_convertible(format!(
                "{path}: not a PNG source (directory or extension)"
            )));
        }
        if nibble && default_mode(dir) != Some(PngMode::Exact) {
            return Err(AssetError::not_convertible(format!(
                "{path}: nibble keys exist only for the colour-keyed directories"
            )));
        }
        let name = if nibble {
            format!("{path}{NK}")
        } else {
            path.to_owned()
        };
        Ok(KeyRoute {
            job: JobKey::new(AssetKind::UiPng, dir, name),
            partition: dir.to_owned(),
        })
    }

    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        PNG_DIRS.contains(&partition).then(|| {
            ctx.kind_fp(
                AssetKind::UiPng,
                BUILDER_VERSION,
                0,
                &[ctx.set_fp(partition, &["tga", "bmp", "dds"])],
            )
        })
    }

    /// Every convertible file in default mode: `theme` first (P2), then `icon`, `item`, `char` (P4), the
    /// 3D directories last (P6); names sorted within a directory.
    fn jobs(&self, ctx: &FpCtx<'_>) -> Vec<JobKey> {
        let mut out = Vec::new();
        for dir in PNG_DIRS {
            let mut names: Vec<&str> = ctx
                .vfs
                .list_dir(dir)
                .into_iter()
                .map(|e| e.path.as_str())
                .filter(|p| is_png_source(p))
                .collect();
            names.sort_unstable();
            out.extend(
                names
                    .into_iter()
                    .map(|p| JobKey::new(AssetKind::UiPng, dir, p)),
            );
        }
        out
    }

    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let (path, nibble) = match job.name.strip_suffix(NK) {
            Some(p) => (p, true),
            None => (job.name.as_str(), false),
        };
        let ext = res::ext_of(path).unwrap_or("");
        let mode = if nibble {
            PngMode::Nibble
        } else {
            default_mode(res::dir_of(path)).unwrap_or(PngMode::None)
        };
        let src = ctx
            .vfs
            .read(path)
            .map_err(|e| AssetError::build(format!("{path}: {e}")))?;
        let bytes =
            convert(ext, &src, mode).map_err(|e| AssetError::build(format!("{path}: {e}")))?;
        let key = if nibble {
            format!("{path}{NK}.png")
        } else {
            format!("{path}.png")
        };
        Ok(vec![BuiltFile::new(key, bytes, "image/png").uncompressed()])
    }
}
