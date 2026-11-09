//! [`CollisionSet`]: the server's collision worlds, one `Arc<CollisionWorld>` per world folder, shared by
//! every world id that uses the folder (`WI_DUNGEON_SECRET_1..16` -> `WdHeaven06_1`, ...), loaded from the
//! cached `FLYC` chunks at start-up (Plan/24 §11.1 step 5c; WP-WEB-14). `ServerDerived.collision` holds it.
//!
//! Loading reads every chunk of every loadable folder through [`AssetPrep::lookup`] (the start-up jobs
//! have just built or found them), inflates and decodes them with `flyff_sim::decode_chunk` on scoped
//! threads and installs them into a `CollisionWorld`. Missing-land failure policy (Plan/24 §7.6): a chunk
//! that is missing or does not decode leaves its land `Missing` (height 0, no objects, the original's
//! "missing land" rule), logs an `error!` and is listed in [`CollisionSet::failures`]; a folder none of
//! whose chunks loaded is left out entirely.

use super::world::{WorldDims, par_map, world_dims};
use crate::error::AssetError;
use crate::kinds::AssetKind;
use crate::prep::AssetPrep;
use flyff_common::defs::world::{WI_GUILDHOUSE_LARGE, WI_GUILDHOUSE_SMALL, WI_WORLD_MINIROOM};
use flyff_data::types::WorldId;
use flyff_sim::collision::chunk::{Attrs, Heights, LandChunk, StaticInstance, VERT_COUNT};
use flyff_sim::collision::world::{CollisionWorld, InitialLands, WorldDesc};
use flyff_sim::decode_chunk;
use std::collections::BTreeMap;
use std::sync::Arc;

/// `true` for the worlds whose static collision includes the dynamic ctrls in under-collision
/// (`WI_WORLD_MINIROOM` and the three guild-house worlds, Plan/31 §10.1 `housing`).
pub fn is_housing_world(id: WorldId) -> bool {
    id == WI_WORLD_MINIROOM || (WI_GUILDHOUSE_SMALL..=WI_GUILDHOUSE_LARGE).contains(&id)
}

/// A chunk (or a whole folder) that could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    /// The world folder.
    pub folder: String,
    /// The land, `None` for the folder as a whole.
    pub land: Option<(u16, u16)>,
    /// What went wrong.
    pub error: String,
}

/// Memory and content statistics of one loaded world (Plan/24 §15.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldStats {
    /// Lands in the world.
    pub lands: u32,
    /// Lands with a decoded chunk.
    pub loaded: u32,
    /// Chunks with `UNIFORM` / `Q16` / `F32` heights.
    pub height_uniform: u32,
    /// See `height_uniform`.
    pub height_q16: u32,
    /// See `height_uniform`.
    pub height_f32: u32,
    /// Chunks with `DENSE_U8` attributes.
    pub attr_dense: u32,
    /// Static instances.
    pub instances: u64,
    /// Meshes over all chunks (not interned across chunks) and their triangles.
    pub meshes: u64,
    /// See `meshes`.
    pub mesh_triangles: u64,
    /// Link cells.
    pub link_cells: u64,
    /// Bytes of heights and attributes in memory.
    pub terrain_bytes: u64,
    /// Bytes of the instance records.
    pub instance_bytes: u64,
    /// Bytes of the mesh vertices and triangles.
    pub mesh_bytes: u64,
}

impl WorldStats {
    /// Measures `world`.
    pub fn of(world: &CollisionWorld) -> WorldStats {
        let mut s = WorldStats {
            lands: world.lands_w * world.lands_h,
            ..WorldStats::default()
        };
        for lz in 0..world.lands_h {
            for lx in 0..world.lands_w {
                let Some(c) = world.chunk(lx, lz) else {
                    continue;
                };
                s.add_chunk(c);
            }
        }
        s
    }

    fn add_chunk(&mut self, c: &LandChunk) {
        self.loaded += 1;
        self.terrain_bytes += match &c.heights {
            Heights::Uniform(_) => {
                self.height_uniform += 1;
                4
            }
            Heights::Q16 { .. } => {
                self.height_q16 += 1;
                2 * VERT_COUNT as u64
            }
            Heights::F32(_) => {
                self.height_f32 += 1;
                4 * VERT_COUNT as u64
            }
        };
        self.terrain_bytes += match &c.attrs {
            Attrs::Uniform(_) => 1,
            Attrs::Dense(_) => {
                self.attr_dense += 1;
                VERT_COUNT as u64
            }
        };
        self.instances += c.instances.len() as u64;
        self.instance_bytes += (c.instances.len() * size_of::<StaticInstance>()) as u64;
        self.link_cells += c.links.len() as u64;
        for m in &c.meshes {
            self.meshes += 1;
            self.mesh_triangles += m.tris.len() as u64;
            self.mesh_bytes += (m.positions.len() * 12 + m.tris.len() * 6) as u64;
        }
    }

    /// Adds `o` into `self`.
    pub fn absorb(&mut self, o: &WorldStats) {
        self.lands += o.lands;
        self.loaded += o.loaded;
        self.height_uniform += o.height_uniform;
        self.height_q16 += o.height_q16;
        self.height_f32 += o.height_f32;
        self.attr_dense += o.attr_dense;
        self.instances += o.instances;
        self.meshes += o.meshes;
        self.mesh_triangles += o.mesh_triangles;
        self.link_cells += o.link_cells;
        self.terrain_bytes += o.terrain_bytes;
        self.instance_bytes += o.instance_bytes;
        self.mesh_bytes += o.mesh_bytes;
    }

    /// Heap bytes of terrain, instances and meshes.
    pub fn memory_bytes(&self) -> u64 {
        self.terrain_bytes + self.instance_bytes + self.mesh_bytes
    }
}

/// The collision worlds of the server (`ServerDerived.collision`, Plan/01 §5.5, 24 §11.1).
#[derive(Default)]
pub struct CollisionSet {
    folders: BTreeMap<String, Arc<CollisionWorld>>,
    worlds: BTreeMap<WorldId, Arc<CollisionWorld>>,
    failures: Vec<LoadFailure>,
}

impl std::fmt::Debug for CollisionSet {
    /// A summary: the worlds hold megabytes of terrain that must not end up in a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollisionSet")
            .field("folders", &self.folders.keys().collect::<Vec<_>>())
            .field("world_ids", &self.worlds.len())
            .field("failures", &self.failures.len())
            .finish()
    }
}

impl CollisionSet {
    /// An empty set.
    pub fn new() -> CollisionSet {
        CollisionSet::default()
    }

    /// Installs `world` for `folder` (lower-case key) and for each of `world_ids`.
    pub fn insert(&mut self, folder: &str, world_ids: &[WorldId], world: Arc<CollisionWorld>) {
        for id in world_ids {
            self.worlds.insert(*id, Arc::clone(&world));
        }
        self.folders.insert(folder.to_owned(), world);
    }

    /// The collision world of a folder (`wdmadrigal`).
    pub fn folder(&self, folder: &str) -> Option<&Arc<CollisionWorld>> {
        self.folders.get(&folder.to_ascii_lowercase())
    }

    /// The collision world of world id `id`: the folder's, shared by every id of the folder.
    pub fn world(&self, id: WorldId) -> Option<&Arc<CollisionWorld>> {
        self.worlds.get(&id)
    }

    /// Every folder with its world, in folder order.
    pub fn folders(&self) -> impl Iterator<Item = (&str, &Arc<CollisionWorld>)> {
        self.folders.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Number of loaded folders.
    pub fn len(&self) -> usize {
        self.folders.len()
    }

    /// `true` when no folder is loaded.
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty()
    }

    /// The chunks and folders that could not be loaded.
    pub fn failures(&self) -> &[LoadFailure] {
        &self.failures
    }

    /// Records a failure.
    pub fn push_failure(&mut self, f: LoadFailure) {
        self.failures.push(f);
    }

    /// Statistics per folder.
    pub fn stats(&self) -> BTreeMap<String, WorldStats> {
        self.folders
            .iter()
            .map(|(k, w)| (k.clone(), WorldStats::of(w)))
            .collect()
    }

    /// Statistics summed over the folders (each folder once, however many ids share it).
    pub fn total_stats(&self) -> WorldStats {
        let mut t = WorldStats::default();
        for w in self.folders.values() {
            t.absorb(&WorldStats::of(w));
        }
        t
    }
}

/// Builds the `CollisionWorld` of `dims` from decoded chunks (missing lands stay `Missing`); a chunk
/// that does not fit the world is returned as an error line.
pub fn world_from_chunks(
    dims: &WorldDims,
    housing: bool,
    chunks: impl IntoIterator<Item = LandChunk>,
) -> Result<(CollisionWorld, Vec<String>), AssetError> {
    let desc = WorldDesc {
        mpu: u32::from(dims.mpu),
        lands_w: u32::from(dims.lands_w),
        lands_h: u32::from(dims.lands_h),
        fly_allowed: dims.fly,
        housing,
    };
    let mut world = CollisionWorld::new(desc, InitialLands::Missing)
        .map_err(|e| AssetError::build(format!("{}: {e}", dims.key)))?;
    let mut rejected = Vec::new();
    for c in chunks {
        let at = (c.land_x, c.land_z);
        if let Err(e) = world.insert_chunk(Arc::new(c)) {
            rejected.push(format!("land {}-{}: {e}", at.0, at.1));
        }
    }
    Ok((world, rejected))
}

/// Loads every loadable world folder from the cache into a [`CollisionSet`] (Plan/24 §11.1 step 5c).
/// `threads == 0` uses the machine's parallelism (at most 8).
pub fn load_collision_set(prep: &AssetPrep, threads: usize) -> Result<CollisionSet, AssetError> {
    let data = Arc::clone(prep.data());
    let mut all_dims: Vec<WorldDims> = Vec::new();
    let mut set = CollisionSet::new();
    for folder in data.worlds.folders.keys() {
        match world_dims(&data, folder) {
            Ok(d) => all_dims.push(d),
            Err(e) => {
                tracing::error!(folder = %folder, error = %e, "collision world skipped");
                set.push_failure(LoadFailure {
                    folder: folder.clone(),
                    land: None,
                    error: e.to_string(),
                });
            }
        }
    }
    // One flat task list over every chunk of every folder: Madrigal's 900 lands dominate, so the work is
    // spread over all folders at once.
    let mut tasks: Vec<(usize, u16, u16)> = Vec::new();
    for (fi, d) in all_dims.iter().enumerate() {
        for z in 0..d.lands_h {
            for x in 0..d.lands_w {
                tasks.push((fi, x, z));
            }
        }
    }
    let threads = if threads == 0 {
        std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1)
            .min(8)
    } else {
        threads
    };
    let decoded = par_map(threads, tasks.len(), &|i| -> Result<LandChunk, String> {
        let &(fi, x, z) = tasks.get(i).ok_or("task index")?;
        let d = all_dims.get(fi).ok_or("folder index")?;
        let key = format!("{}/{x}-{z}.bin", d.key);
        let cf = match prep.lookup(AssetKind::CollisionChunk, &key) {
            Ok(Some(cf)) => cf,
            Ok(None) => return Err("not in the cache".to_owned()),
            Err(e) => return Err(e.to_string()),
        };
        let bytes = cf.read_decoded().map_err(|e| format!("read: {e}"))?;
        let chunk = decode_chunk(&bytes).map_err(|e| format!("decode: {e}"))?;
        if (chunk.land_x, chunk.land_z) != (x, z) {
            return Err(format!(
                "the chunk says it is land {}-{}",
                chunk.land_x, chunk.land_z
            ));
        }
        Ok(chunk)
    });

    let mut per_folder: Vec<Vec<LandChunk>> = all_dims.iter().map(|_| Vec::new()).collect();
    let mut failed_lands: Vec<usize> = vec![0; all_dims.len()];
    for ((fi, x, z), r) in tasks.iter().zip(decoded) {
        match r {
            Ok(c) => {
                if let Some(v) = per_folder.get_mut(*fi) {
                    v.push(c);
                }
            }
            Err(e) => {
                let folder = all_dims.get(*fi).map(|d| d.key.clone()).unwrap_or_default();
                tracing::error!(%folder, x = *x, z = *z, error = %e, "collision chunk not loaded; the land is missing");
                if let Some(n) = failed_lands.get_mut(*fi) {
                    *n += 1;
                }
                set.push_failure(LoadFailure {
                    folder,
                    land: Some((*x, *z)),
                    error: e,
                });
            }
        }
    }
    for (dims, chunks) in all_dims.iter().zip(per_folder) {
        let lands = usize::from(dims.lands_w) * usize::from(dims.lands_h);
        if chunks.is_empty() && lands > 0 {
            tracing::error!(folder = %dims.key, "no collision chunk of the folder loaded; the world is not available");
            set.push_failure(LoadFailure {
                folder: dims.key.clone(),
                land: None,
                error: format!("none of the {lands} chunks could be loaded"),
            });
            continue;
        }
        let housing = dims.world_ids.iter().copied().any(is_housing_world);
        let (world, rejected) = world_from_chunks(dims, housing, chunks)?;
        for line in rejected {
            tracing::error!(folder = %dims.key, "{line}");
            set.push_failure(LoadFailure {
                folder: dims.key.clone(),
                land: None,
                error: line,
            });
        }
        set.insert(&dims.key, &dims.world_ids, Arc::new(world));
    }
    Ok(set)
}
