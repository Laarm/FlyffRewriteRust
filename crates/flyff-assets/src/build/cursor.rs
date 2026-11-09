//! Cursors (`cursor` kind, Plan/24 §10.3, WP-WEB-08): `cursor/*.cur` become PNGs plus a hotspot in the
//! client manifest.
//!
//! * **Entry choice:** the 32x32 entry; else the largest entry up to 128x128 (browsers cap cursor images
//!   at 128x128); else the first.
//! * **Pixels:** colour from the XOR image; alpha = the 32-bpp alpha when the image has any non-zero
//!   alpha, else the AND mask (`1` transparent, `0` opaque). All of that is `flyff_formats::cur`'s decoder;
//!   an inverting pixel (AND 1, XOR != 0), which has no CSS equivalent, is drawn opaque in its XOR colour
//!   there (none of the 23 shipped cursors has one).
//! * **Output:** `gen/cursor/<fp>/<stem>.png` (the PNG of [`super::png`], `Compress::Never`). The hotspot and
//!   size of the chosen entry go to `ManifestParts::set_cursor` during `on_start`, i.e. in
//!   `ClientManifest.cursors[<stem>]` (key = lower-case file stem).

use super::png::encode_png;
use crate::cache::Cache;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{AssetBuilder, AssetKind, BuildCtx, BuiltFile, JobKey, KeyRoute, NO_PARTITION};
use crate::manifest::CursorInfo;
use crate::res;
use flyff_formats::cur::{self, CursorImage};

/// Bump on any change that alters the produced bytes.
pub const BUILDER_VERSION: u32 = 1;

/// The largest cursor image a browser draws.
pub const MAX_CURSOR_SIDE: u32 = 128;

/// The lower-case stem of `cursor/<stem>.cur`, `None` for any other path.
fn stem_of(path: &str) -> Option<&str> {
    if res::dir_of(path) != "cursor" || res::ext_of(path) != Some("cur") {
        return None;
    }
    path.rsplit('/')
        .next()?
        .strip_suffix(".cur")
        .filter(|s| !s.is_empty())
}

/// Picks the entry of a parsed `.cur` per §10.3.
pub fn choose_entry(images: &[CursorImage]) -> Option<&CursorImage> {
    images
        .iter()
        .find(|i| i.image.width == 32 && i.image.height == 32)
        .or_else(|| {
            images
                .iter()
                .filter(|i| i.image.width <= MAX_CURSOR_SIDE && i.image.height <= MAX_CURSOR_SIDE)
                .max_by_key(|i| u64::from(i.image.width) * u64::from(i.image.height))
        })
        .or_else(|| images.first())
}

/// Parses a `.cur` and returns the chosen entry's hotspot, size and PNG.
pub fn convert(bytes: &[u8]) -> Result<(CursorInfo, Vec<u8>), String> {
    let parsed = cur::parse(bytes).map_err(|e| e.to_string())?;
    let entry = choose_entry(&parsed.value).ok_or_else(|| "no image".to_owned())?;
    let png = encode_png(entry.image.width, entry.image.height, &entry.image.pixels)?;
    let info = CursorInfo {
        hotspot: [u32::from(entry.hotspot.0), u32::from(entry.hotspot.1)],
        size: [entry.image.width, entry.image.height],
    };
    Ok((info, png))
}

/// The builder of kind [`AssetKind::CursorPng`]: one job per `.cur`, no partitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct CursorBuilder;

impl AssetBuilder for CursorBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::CursorPng
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    /// `<lower stem>.png`.
    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let stem = key
            .strip_suffix(".png")
            .filter(|s| !s.is_empty() && !s.contains('/'))
            .ok_or_else(|| AssetError::bad_key(format!("{key:?}: a cursor key is <stem>.png")))?;
        let path = format!("cursor/{stem}.cur");
        let entry = ctx
            .vfs
            .resolve(&path)
            .ok_or_else(|| AssetError::not_found(format!("no such cursor: {path}")))?;
        if entry.path.as_str() != path {
            return Err(AssetError::bad_key(format!(
                "not a canonical stem: {stem:?}"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::single(AssetKind::CursorPng, stem),
            partition: NO_PARTITION.to_owned(),
        })
    }

    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        (partition == NO_PARTITION).then(|| {
            ctx.kind_fp(
                AssetKind::CursorPng,
                BUILDER_VERSION,
                0,
                &[ctx.set_fp("cursor", &["cur"])],
            )
        })
    }

    /// Every `cursor/*.cur`, sorted by stem (prewarm step P2).
    fn jobs(&self, ctx: &FpCtx<'_>) -> Vec<JobKey> {
        let mut stems: Vec<&str> = ctx
            .vfs
            .list_dir("cursor")
            .into_iter()
            .filter_map(|e| stem_of(e.path.as_str()))
            .collect();
        stems.sort_unstable();
        stems
            .into_iter()
            .map(|s| JobKey::single(AssetKind::CursorPng, s))
            .collect()
    }

    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let path = format!("cursor/{}.cur", job.name);
        let src = ctx
            .vfs
            .read(&path)
            .map_err(|e| AssetError::build(format!("{path}: {e}")))?;
        let (_, png) = convert(&src).map_err(|e| AssetError::build(format!("{path}: {e}")))?;
        Ok(vec![
            BuiltFile::new(format!("{}.png", job.name), png, "image/png").uncompressed(),
        ])
    }

    /// Parses the headers of every cursor (`ClientManifest.cursors`). A broken file is logged and left
    /// out; its job fails when somebody asks for it.
    fn on_start(&self, ctx: &BuildCtx<'_>, _cache: &Cache) -> Result<(), AssetError> {
        for e in ctx.vfs.list_dir("cursor") {
            let Some(stem) = stem_of(e.path.as_str()) else {
                continue;
            };
            match ctx.vfs.read(e.path.as_str()) {
                Ok(bytes) => match cur::parse(&bytes) {
                    Ok(p) => {
                        if let Some(entry) = choose_entry(&p.value) {
                            ctx.manifest_parts.set_cursor(
                                stem,
                                CursorInfo {
                                    hotspot: [
                                        u32::from(entry.hotspot.0),
                                        u32::from(entry.hotspot.1),
                                    ],
                                    size: [entry.image.width, entry.image.height],
                                },
                            );
                        }
                    }
                    Err(err) => tracing::warn!(file = %e.path, error = %err, "unreadable cursor"),
                },
                Err(err) => tracing::warn!(file = %e.path, error = %err, "unreadable cursor"),
            }
        }
        Ok(())
    }
}
