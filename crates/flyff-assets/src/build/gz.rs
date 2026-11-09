//! Gzip variants of raw assets (Plan/24 §10.1, WP-WEB-10).
//!
//! * **Eligible:** a `/res`-servable file ([`crate::res`]) with extension in `o3d chr ani lnd sfx dds tga
//!   bmp wav cur` and size >= 1 KiB. Never `ogg mp3 png jpg` (already compressed) and nothing under
//!   `music/`.
//! * **Build:** `flate2::GzBuilder` (no file name, mtime 0, OS byte 255), level 9, over the whole file.
//!   Keep iff `gz_len < 0.9 * raw_len` as `res-gz/<path>.<file_fp>.gz`; otherwise write the empty marker
//!   `res-gz/<path>.<file_fp>.nogz` so the decision is not recomputed until the file changes.
//! * **Serving:** `AssetPrep::res_gz` / `request_gz` implement §5.3 step 6; this module also provides
//!   the content-negotiation helpers ([`accepts_gzip`], [`choose_representation`]).
//! * **Prewarm** (§7.5): P3 `.lnd` (`wdmadrigal` first), P5 models, model textures (non-fix), sfx,
//!   world/texture, weather, P7 `sound/*.wav`. Prewarm stops enqueuing at 95 % of the budget, lazy builds
//!   at 100 %; `assets.gzip_raw = "lazy"` removes the prewarm, `"off"` disables the variants.

use crate::cache::{GZ_MIN_RAW, gzip_beneficial, gzip_bytes, parse_res_gz};
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx, file_fp};
use crate::kinds::{
    AssetBuilder, AssetKind, BuildCtx, BuiltFile, Compress, JobKey, KeyRoute, PrewarmStep,
    content_type_by_ext,
};
use crate::manifest::ManifestParts;
use crate::res::{self, DirPattern};
use std::sync::Arc;

/// Bump on any change that alters the produced bytes (level, header fields).
pub const BUILDER_VERSION: u32 = 1;

/// The deflate level of the variants (the best ratio; built in the background).
pub const GZ_LEVEL: u32 = 9;

/// Extensions that get a variant (Plan/24 §10.1).
pub const ELIGIBLE_EXTS: &[&str] = &[
    "o3d", "chr", "ani", "lnd", "sfx", "dds", "tga", "bmp", "wav", "cur",
];

/// Is the normalised path `path` of `size` bytes eligible for a gzip variant?
pub fn is_eligible(path: &str, size: u64) -> bool {
    size >= GZ_MIN_RAW
        && res::ext_of(path).is_some_and(|e| ELIGIBLE_EXTS.contains(&e))
        && res::dir_of(path) != "music"
        && res::is_client_visible(path)
}

/// The prewarm step of a gzip job (`None` = not prewarmed: converted formats, minimaps, cursors).
/// The `dds_fix` exclusion is applied by [`GzBuilder::jobs`], which knows the fix list.
pub fn prewarm_step(path: &str) -> Option<PrewarmStep> {
    let dir = res::dir_of(path);
    let ext = res::ext_of(path)?;
    match (dir, ext) {
        (d, "lnd") if DirPattern::WorldFolder.matches(d) => Some(PrewarmStep::P3),
        ("model", "o3d" | "chr" | "ani") => Some(PrewarmStep::P5),
        ("model/texture" | "world/texture" | "weather" | "sfx/texture", "dds") => {
            Some(PrewarmStep::P5)
        }
        ("sfx", "sfx" | "dds") | ("sfx/texture", "sfx") => Some(PrewarmStep::P5),
        ("sound", "wav") => Some(PrewarmStep::P7),
        _ => None,
    }
}

/// The order inside prewarm step P5 (Plan/24 §7.5): models, model textures, sfx, world textures,
/// weather. The sort is stable, so files of one class stay in path order.
fn p5_rank(path: &str) -> u8 {
    match res::dir_of(path) {
        "model" => 0,
        "model/texture" => 1,
        "sfx" | "sfx/texture" => 2,
        "world/texture" => 3,
        _ => 4, // weather
    }
}

/// `res-gz/` file name (relative to the directory) of a variant or its marker.
pub fn variant_name(path: &str, fp: Fp, gz: bool) -> String {
    format!("{path}.{fp}.{}", if gz { "gz" } else { "nogz" })
}

/// Does an `Accept-Encoding` header allow `gzip`? An explicit `gzip`/`x-gzip` entry decides (`q=0`
/// refuses); otherwise `*` with a non-zero weight accepts.
pub fn accepts_gzip(accept_encoding: Option<&str>) -> bool {
    let Some(h) = accept_encoding else {
        return false;
    };
    let mut star = None;
    for item in h.split(',') {
        let mut parts = item.split(';');
        let coding = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut weight = 1.0f32;
        for p in parts {
            if let Some(q) = p
                .trim()
                .strip_prefix("q=")
                .or_else(|| p.trim().strip_prefix("Q="))
            {
                weight = q.trim().parse::<f32>().unwrap_or(0.0);
            }
        }
        match coding.as_str() {
            "gzip" | "x-gzip" => return weight > 0.0,
            "*" => star = Some(weight > 0.0),
            _ => {}
        }
    }
    star.unwrap_or(false)
}

/// Which form of a raw file a response uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Representation {
    /// The raw file.
    Identity,
    /// The stored gzip variant, served with `Content-Encoding: gzip`.
    Gzip,
}

/// §5.3 step 6: a `Range` request is always identity; otherwise the gzip variant when it exists and the
/// client accepts it.
pub fn choose_representation(
    state: &crate::prep::GzState,
    accept_encoding: Option<&str>,
    has_range: bool,
) -> Representation {
    if !has_range
        && matches!(state, crate::prep::GzState::Present(_))
        && accepts_gzip(accept_encoding)
    {
        Representation::Gzip
    } else {
        Representation::Identity
    }
}

/// The builder of kind [`AssetKind::Gzip`]. One job per raw file; `JobKey { partition: <dir>, name:
/// <vfs path> }`.
pub struct GzBuilder {
    parts: Arc<ManifestParts>,
}

impl GzBuilder {
    /// A builder that skips the DDS files listed in `parts` when planning the prewarm.
    pub fn new(parts: Arc<ManifestParts>) -> Self {
        Self { parts }
    }
}

impl AssetBuilder for GzBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::Gzip
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    /// The key is the canonical VFS path of the raw file.
    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let norm = ctx
            .vfs
            .resolve(key)
            .ok_or_else(|| AssetError::not_found(format!("no such file: {key:?}")))?;
        if norm.path.as_str() != key {
            return Err(AssetError::bad_key(format!(
                "not a canonical path: {key:?}"
            )));
        }
        if !is_eligible(key, norm.size) {
            return Err(AssetError::not_convertible(format!(
                "{key}: not gzip-eligible"
            )));
        }
        let dir = res::dir_of(key).to_owned();
        Ok(KeyRoute {
            job: JobKey::new(AssetKind::Gzip, dir.clone(), key),
            partition: dir,
        })
    }

    /// The group fingerprint of the directory (the per-file fingerprint names the variant).
    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        ctx.group_fp(partition)
    }

    /// The prewarm set in P3, P5, P7 order (§7.5): every eligible file of the three classes, `.lnd` of
    /// `wdmadrigal` first, `dds_fix` files excluded.
    fn jobs(&self, ctx: &FpCtx<'_>) -> Vec<JobKey> {
        let (mut p3_first, mut p3, mut p5, mut p7) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for e in ctx.vfs.iter() {
            let path = e.path.as_str();
            let Some(step) = prewarm_step(path) else {
                continue;
            };
            if !is_eligible(path, e.size) || self.parts.is_dds_fix(path) {
                continue;
            }
            let job = JobKey::new(AssetKind::Gzip, res::dir_of(path), path);
            match step {
                PrewarmStep::P3 if res::dir_of(path) == "world/wdmadrigal" => p3_first.push(job),
                PrewarmStep::P3 => p3.push(job),
                PrewarmStep::P7 => p7.push(job),
                _ => p5.push(job),
            }
        }
        // §7.5 P5: models, then model textures, then sfx, then world/texture, then weather.
        p5.sort_by_key(|j| p5_rank(&j.name));
        p3_first.into_iter().chain(p3).chain(p5).chain(p7).collect()
    }

    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let entry = ctx
            .vfs
            .resolve(&job.name)
            .ok_or_else(|| AssetError::not_found(format!("no such file: {}", job.name)))?;
        let fp = file_fp(entry);
        let raw = ctx
            .vfs
            .read(&job.name)
            .map_err(|e| AssetError::build(format!("{}: {e}", job.name)))?;
        if raw.len() as u64 != entry.size {
            return Err(AssetError::build(format!(
                "{}: size changed since the index was built ({} -> {} bytes)",
                job.name,
                entry.size,
                raw.len()
            )));
        }
        let gz = gzip_bytes(&raw, GZ_LEVEL);
        let ct = content_type_by_ext(&job.name);
        let file = if gzip_beneficial(raw.len() as u64, gz.len() as u64) {
            BuiltFile {
                key: variant_name(&job.name, fp, true),
                bytes: gz,
                content_type: ct,
                compress: Compress::Never,
            }
        } else {
            BuiltFile {
                key: variant_name(&job.name, fp, false),
                bytes: Vec::new(),
                content_type: ct,
                compress: Compress::Never,
            }
        };
        Ok(vec![file])
    }

    fn content_type(&self, key: &str) -> &'static str {
        parse_res_gz(key).map_or("application/octet-stream", |(p, _, _)| {
            content_type_by_ext(p)
        })
    }
}
