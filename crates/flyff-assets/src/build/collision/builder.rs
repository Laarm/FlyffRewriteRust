//! The `collision` asset kind (Plan/24 §7.1, §9.2, §10.7, §11.1 step 5c; WP-WEB-14): one start-up job per
//! loadable world folder writes every land's `FLYC` chunk (`<folder>/<x>-<z>.bin`) plus a small
//! `<folder>/report.json` with the build counters; the start-up hook decodes the chunks into
//! `ServerDerived.collision`.

use super::BUILDER_VERSION;
use super::FORMAT_VERSION;
use super::assemble::CollisionReport;
use super::set::load_collision_set;
use super::world::{BuildOptions, build_world_chunks_with, present_lands, world_dims};
use crate::cache::Cache;
use crate::derived::ServerDerived;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{AssetBuilder, AssetKind, BuildCtx, BuiltFile, Compress, JobKey, KeyRoute};
use crate::manifest::WorldInfo;
use crate::prep::AssetPrep;
use crate::util::lock;
use flyff_vfs::Vfs;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The compression level of the start-up collision job (Plan/24 §8.2: 6 for Startup collision).
pub const COLLISION_GZIP_LEVEL: u32 = 6;

/// The `.bin` MIME type.
const CHUNK_MIME: &str = "application/octet-stream";

/// The key of the per-folder build report inside the partition (`<folder>/report.json`).
pub fn report_key(folder: &str) -> String {
    format!("{folder}/report.json")
}

/// The key of land `(x, z)` (`<folder>/<x>-<z>.bin`, decimal without padding, Plan/24 §9.2).
pub fn chunk_key(folder: &str, x: u16, z: u16) -> String {
    format!("{folder}/{x}-{z}.bin")
}

/// Fingerprint inputs that are the same for every partition, memoised per VFS (the VFS is static for
/// the process lifetime, so the stat-based sets are computed once instead of per lookup).
#[derive(Default)]
struct FpCache {
    /// Identity of the VFS the values belong to (address and entry count).
    vfs: (usize, usize),
    model: Option<Fp>,
    registry: Option<Fp>,
    folders: BTreeMap<String, Fp>,
}

/// The `collision` builder.
pub struct CollisionBuilder {
    threads: usize,
    cache: Mutex<FpCache>,
}

impl Default for CollisionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CollisionBuilder {
    /// A builder whose jobs use the machine's parallelism (at most 8 threads each).
    pub fn new() -> CollisionBuilder {
        CollisionBuilder {
            threads: 0,
            cache: Mutex::new(FpCache::default()),
        }
    }

    /// A builder whose jobs use exactly `threads` worker threads (`0` = automatic).
    #[must_use]
    pub fn with_threads(mut self, threads: usize) -> CollisionBuilder {
        self.threads = threads;
        self
    }

    fn vfs_identity(vfs: &Vfs) -> (usize, usize) {
        (std::ptr::from_ref(vfs) as usize, vfs.len())
    }

    /// `kind_fp` of the folder partition (Plan/24 §6.3): `set_fp("world/<f>", ["lnd","wld"])`,
    /// `set_fp("model", ["o3d"])`, `registry_fp`.
    fn folder_fp(&self, ctx: &FpCtx<'_>, folder: &str) -> Fp {
        let mut c = lock(&self.cache);
        let id = Self::vfs_identity(ctx.vfs);
        if c.vfs != id {
            *c = FpCache {
                vfs: id,
                ..FpCache::default()
            };
        }
        let model = *c.model.get_or_insert_with(|| ctx.set_fp("model", &["o3d"]));
        let registry = *c.registry.get_or_insert_with(|| ctx.registry_fp());
        let world = *c
            .folders
            .entry(folder.to_owned())
            .or_insert_with(|| ctx.set_fp(&format!("world/{folder}"), &["lnd", "wld"]));
        ctx.kind_fp(
            AssetKind::CollisionChunk,
            BUILDER_VERSION,
            FORMAT_VERSION,
            &[world, model, registry],
        )
    }
}

/// Parses `<x>-<z>.bin` (decimal, no sign, no padding).
fn parse_land_name(file: &str) -> Option<(u32, u32)> {
    let stem = file.strip_suffix(".bin")?;
    let (x, z) = stem.split_once('-')?;
    let num = |s: &str| -> Option<u32> {
        let canonical = !s.is_empty()
            && s.len() <= 5
            && s.bytes().all(|b| b.is_ascii_digit())
            && (s == "0" || !s.starts_with('0'));
        if canonical { s.parse().ok() } else { None }
    };
    Some((num(x)?, num(z)?))
}

impl AssetBuilder for CollisionBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::CollisionChunk
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        FORMAT_VERSION
    }

    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let bad = || {
            AssetError::bad_key(format!(
                "collision keys are <folder>/<x>-<z>.bin or <folder>/report.json: {key:?}"
            ))
        };
        let (folder, file) = key.split_once('/').ok_or_else(bad)?;
        if folder.is_empty() || file.contains('/') {
            return Err(bad());
        }
        let land = if file == "report.json" {
            None
        } else {
            Some(parse_land_name(file).ok_or_else(bad)?)
        };
        let info =
            ctx.data.worlds.folders.get(folder).ok_or_else(|| {
                AssetError::not_found(format!("no loadable world folder {folder:?}"))
            })?;
        if let Some((x, z)) = land {
            let (w, h) = info.settings.size;
            if x >= w || z >= h {
                return Err(AssetError::not_found(format!(
                    "land {x}-{z} is outside the {w} x {h} world {folder:?}"
                )));
            }
        }
        Ok(KeyRoute {
            job: JobKey::new(AssetKind::CollisionChunk, folder, folder),
            partition: folder.to_owned(),
        })
    }

    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        if !ctx.data.worlds.folders.contains_key(partition) {
            return None;
        }
        Some(self.folder_fp(ctx, partition))
    }

    fn jobs(&self, ctx: &FpCtx<'_>) -> Vec<JobKey> {
        // The biggest worlds first: Madrigal (900 lands) is the long pole of the start-up stage.
        let mut folders: Vec<(&String, u32)> = ctx
            .data
            .worlds
            .folders
            .iter()
            .map(|(k, f)| (k, f.settings.size.0.saturating_mul(f.settings.size.1)))
            .collect();
        folders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        folders
            .into_iter()
            .map(|(k, _)| JobKey::new(AssetKind::CollisionChunk, k.as_str(), k.as_str()))
            .collect()
    }

    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let folder = job.partition.as_str();
        let opts = BuildOptions {
            threads: self.threads,
        };
        let (chunks, report) =
            build_world_chunks_with(ctx.vfs, ctx.data, folder, None, &opts, &|| ctx.cancelled())?;
        tracing::debug!(
            folder,
            chunks = report.chunks,
            instances = report.instances,
            triangles = report.triangles,
            bytes = report.bytes,
            failed_lands = report.lands_failed,
            "collision world built"
        );
        let compress = Compress::IfBeneficial {
            level: COLLISION_GZIP_LEVEL,
        };
        let mut files = Vec::with_capacity(chunks.len() + 1);
        for ((x, z), bytes) in chunks {
            files.push(BuiltFile {
                key: chunk_key(folder, x, z),
                bytes,
                content_type: CHUNK_MIME,
                compress,
            });
        }
        files.push(BuiltFile {
            key: report_key(folder),
            bytes: report_json(&report),
            content_type: "application/json",
            compress,
        });
        Ok(files)
    }

    fn on_start(&self, ctx: &BuildCtx<'_>, _cache: &Cache) -> Result<(), AssetError> {
        // §11.1 step 4: the loadable worlds for `ClientManifest.worlds` (stat data only).
        for folder in ctx.data.worlds.folders.keys() {
            match world_dims(ctx.data, folder) {
                Ok(d) => ctx.manifest_parts.set_world(
                    &d.key,
                    WorldInfo {
                        lands: [u32::from(d.lands_w), u32::from(d.lands_h)],
                        mpu: u32::from(d.mpu),
                        present: present_lands(ctx.vfs, &d),
                    },
                ),
                Err(e) => {
                    tracing::error!(folder = %folder, error = %e, "world not listed in the manifest")
                }
            }
        }
        Ok(())
    }

    fn load_server(&self, prep: &AssetPrep, out: &mut ServerDerived) -> Result<(), AssetError> {
        let set = load_collision_set(prep, self.threads)?;
        let stats = set.total_stats();
        tracing::info!(
            worlds = set.len(),
            chunks = stats.loaded,
            instances = stats.instances,
            memory_mb = stats.memory_bytes() >> 20,
            failures = set.failures().len(),
            "collision worlds loaded"
        );
        out.collision = Some(Arc::new(set));
        Ok(())
    }
}

/// The compact deterministic JSON of a report (what `<folder>/report.json` stores).
pub fn report_json(report: &CollisionReport) -> Vec<u8> {
    serde_json::to_vec(report).unwrap_or_default()
}
