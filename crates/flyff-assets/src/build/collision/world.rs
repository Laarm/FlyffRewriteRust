//! `build_world_chunks`: the `FLYC` chunks of one world folder from the VFS, the format parsers and the
//! game data, without the asset framework (Plan/24 §10.7 "Build paths"; WP-WEB-24).
//!
//! The function reads, in the order of the original's `CWorld::ReadWorld` (src:_Common/
//! WorldFile.cpp:823-955): the world settings from the `WorldRegistry` (`lands_w`, `lands_h`, `mpu`), every
//! land's `.lnd` (heights, water, object table) through `flyff-formats`, the model of every `OT_OBJ` record
//! through the `ModelRegistry` and the collision body of its `.o3d`, then assembles
//! ([`super::assemble`]) and encodes ([`flyff_sim::encode_chunk`]) the lands. Lands and models are parsed
//! and chunks assembled and encoded on scoped threads ([`BuildOptions::threads`]); the output does not
//! depend on the thread count.
//!
//! Failure policy (Plan/24 §7.6): a land whose `.lnd` cannot be read becomes a missing land (uniform zero,
//! no objects) with an `error!` and an entry in [`CollisionReport::failures`]; a model whose `.o3d` cannot
//! be used loses its instances (the original's `LoadModel` returned NULL), also reported. Only problems of
//! the world itself (unknown folder, impossible `.wld` values) are errors.

use super::assemble::{
    CollisionInputs, CollisionMesh, CollisionReport, LandInput, ObjectInput, WorldAssembly,
    instance_flags,
};
use crate::error::AssetError;
use flyff_common::defs::define::{MODELTYPE_ANIMATED_MESH, MODELTYPE_MESH};
use flyff_data::GameData;
use flyff_data::types::WorldId;
use flyff_data::worlds::WorldRegistry;
use flyff_formats::error::WarningCode;
use flyff_formats::lnd::{HEIGHT_COUNT, LandView};
use flyff_formats::o3d::{self, O3dOptions, Vertices};
use flyff_formats::objects::OT_OBJ;
use flyff_sim::collision::chunk::PATCH_COUNT;
use flyff_sim::collision::world::MAX_WORLD_LANDS;
use flyff_sim::encode_chunk;
use flyff_vfs::Vfs;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Component-wise start values of the original's model box (`CModelObject::Init`, `m_vMin = +65535`,
/// `m_vMax = -65535`, src:_Common/ModelObject.cpp:77-78): the box of a single element is the element's
/// o3d box clamped by them.
const EMPTY_BOX: f32 = 65535.0;

/// Options of [`build_world_chunks_with`].
#[derive(Clone, Copy, Debug, Default)]
pub struct BuildOptions {
    /// Worker threads for parsing and encoding; `0` = the machine's parallelism (at most 8). Small
    /// worlds always run on one thread. The output is identical for every value.
    pub threads: usize,
}

/// The world settings the builder needs (`WorldRegistry` + `.wld`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldDims {
    /// The folder key (lower case, as in `WorldRegistry::folders`).
    pub key: String,
    /// World width in lands.
    pub lands_w: u16,
    /// World height in lands.
    pub lands_h: u16,
    /// World units per heightmap cell (1, 2 or 4).
    pub mpu: u8,
    /// `.wld` `fly` flag.
    pub fly: bool,
    /// The world ids that use the folder, ascending.
    pub world_ids: Vec<WorldId>,
}

/// The settings of a loadable world folder; errors for an unknown folder or impossible `.wld` values.
pub fn world_dims(data: &GameData, folder: &str) -> Result<WorldDims, AssetError> {
    let key = WorldRegistry::folder_key(folder);
    let f = data.worlds.folders.get(&key).ok_or_else(|| {
        AssetError::not_found(format!("world folder {folder:?} is not a loadable world"))
    })?;
    let s = &f.settings;
    let (w, h) = s.size;
    if w == 0 || h == 0 || w > MAX_WORLD_LANDS || h > MAX_WORLD_LANDS {
        return Err(AssetError::build(format!(
            "{key}.wld: size {w} x {h} is outside 1..={MAX_WORLD_LANDS} lands"
        )));
    }
    let mpu = u8::try_from(s.mpu)
        .ok()
        .filter(|m| matches!(m, 1 | 2 | 4))
        .ok_or_else(|| AssetError::build(format!("{key}.wld: MPU {} is not 1, 2 or 4", s.mpu)))?;
    Ok(WorldDims {
        key,
        // BOUNDS: both are at most MAX_WORLD_LANDS = 256, checked above
        lands_w: u16::try_from(w).unwrap_or(u16::MAX),
        lands_h: u16::try_from(h).unwrap_or(u16::MAX),
        mpu,
        fly: s.fly,
        world_ids: f.world_ids.clone(),
    })
}

/// The VFS path of land `(x, z)`: `world/<key>/<key><xx>-<zz>.lnd` (`%02d` as in the original).
pub fn lnd_path(key: &str, x: u16, z: u16) -> String {
    format!("world/{key}/{key}{x:02}-{z:02}.lnd")
}

/// `x + z * lands_w` of every land whose `.lnd` exists (the manifest's `worlds[folder].present`).
pub fn present_lands(vfs: &Vfs, dims: &WorldDims) -> Vec<u32> {
    let mut out = Vec::new();
    for z in 0..dims.lands_h {
        for x in 0..dims.lands_w {
            if vfs.exists(&lnd_path(&dims.key, x, z)) {
                out.push(u32::from(z) * u32::from(dims.lands_w) + u32::from(x));
            }
        }
    }
    out
}

/// The default worker count: the machine's parallelism, at most 8.
fn auto_threads() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .min(8)
}

/// Runs `f(0..n)` on up to `threads` scoped threads and returns the results in index order. A panic of
/// a worker is re-raised on the caller (the asset pool turns it into a build error).
pub(crate) fn par_map<R: Send>(
    threads: usize,
    n: usize,
    f: &(dyn Fn(usize) -> R + Sync),
) -> Vec<R> {
    let threads = threads.clamp(1, n.max(1));
    if threads == 1 {
        return (0..n).map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut all: Vec<(usize, R)> = Vec::with_capacity(n);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            break;
                        }
                        mine.push((i, f(i)));
                    }
                    mine
                })
            })
            .collect();
        for h in handles {
            match h.join() {
                Ok(part) => all.extend(part),
                Err(payload) => resume_unwind(payload),
            }
        }
    });
    all.sort_by_key(|(i, _)| *i);
    all.into_iter().map(|(_, r)| r).collect()
}

/// One object record before its model is resolved.
struct RawObject {
    ordinal: u32,
    obj_type: u32,
    model_index: u32,
    pos_file: [f32; 3],
    angles: [f32; 3],
    scale: [f32; 3],
}

/// One land read from its file.
struct LandRead {
    present: bool,
    /// Why the file was unusable (`present` is then `false`).
    failure: Option<String>,
    water: [u8; 2 * PATCH_COUNT],
    heights: Option<Box<[f32]>>,
    objects: Vec<RawObject>,
}

impl LandRead {
    fn missing() -> LandRead {
        LandRead {
            present: false,
            failure: None,
            water: [0; 2 * PATCH_COUNT],
            heights: None,
            objects: Vec::new(),
        }
    }

    fn failed(why: String) -> LandRead {
        LandRead {
            failure: Some(why),
            ..LandRead::missing()
        }
    }
}

/// Reads one `.lnd` through the VFS: heights (only when `want_terrain`), water and the object table.
fn read_land(vfs: &Vfs, path: &str, want_terrain: bool) -> LandRead {
    if !vfs.exists(path) {
        return LandRead::missing();
    }
    let bytes = match vfs.read(path) {
        Ok(b) => b,
        Err(e) => return LandRead::failed(format!("{path}: {e}")),
    };
    let view = match LandView::parse(&bytes) {
        Ok(v) => v,
        Err(e) => return LandRead::failed(format!("{path}: {e}")),
    };
    let mut objects = Vec::with_capacity(view.object_count());
    for (i, rec) in view.objects().enumerate() {
        match rec {
            Ok(r) => objects.push(RawObject {
                ordinal: u32::try_from(i).unwrap_or(u32::MAX),
                obj_type: r.obj_type,
                model_index: r.base.index,
                pos_file: r.base.pos_stored,
                angles: [r.base.angle, r.base.axis[0], r.base.axis[2]],
                scale: r.base.scale,
            }),
            Err(e) => return LandRead::failed(format!("{path}: object {i}: {e}")),
        }
    }
    let mut water = [0u8; 2 * PATCH_COUNT];
    let wb = view.water_bytes();
    if wb.len() == water.len() {
        water.copy_from_slice(wb);
    }
    let heights = want_terrain.then(|| {
        let mut h = Vec::with_capacity(HEIGHT_COUNT);
        h.extend(view.heights());
        h.into_boxed_slice()
    });
    LandRead {
        present: true,
        failure: None,
        water,
        heights,
        objects,
    }
}

/// What a model index resolves to.
enum ModelState {
    /// A collision body and the instance flags of the model.
    Mesh(Arc<CollisionMesh>, u8),
    /// The index is not in `mdlObj.inc`.
    Unknown,
    /// A billboard, sfx or ASE model: `LoadModel` produces no mesh.
    NonMesh,
    /// The model has no collision body.
    NoCollision,
    /// The `.o3d` is missing or unusable.
    Unreadable(String),
}

/// Loads the collision body of `mdlObj` entry `index` (Plan/24 §10.7.4): positions and triangles of the
/// o3d collision body in file order, the header box clamped like `CModelObject::LoadElement`.
fn load_model(vfs: &Vfs, data: &GameData, index: u32) -> ModelState {
    let Some(elem) = data.models.get(OT_OBJ, index) else {
        return ModelState::Unknown;
    };
    if elem.model_type != MODELTYPE_MESH && elem.model_type != MODELTYPE_ANIMATED_MESH {
        return ModelState::NonMesh;
    }
    let Some(file) = data.models.model_file(OT_OBJ, index) else {
        return ModelState::Unknown;
    };
    let file = file.to_ascii_lowercase();
    let path = format!("model/{file}");
    let bytes = match vfs.read(&path) {
        Ok(b) => b,
        Err(e) => return ModelState::Unreadable(format!("{path}: {e}")),
    };
    let parsed = match o3d::parse(&bytes, &O3dOptions::skip().with_file_name(&file)) {
        Ok(p) => p,
        Err(e) => return ModelState::Unreadable(format!("{path}: {e}")),
    };
    // KEEP-QUIRK (Plan/30 Q1, KQ-42): a header name that differs from the requested file name makes the
    // original refuse to load the model.
    if parsed
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::O3dNameMismatch)
    {
        return ModelState::Unreadable(format!("{path}: header name does not match the file name"));
    }
    let o = parsed.value;
    let Some(body) = &o.collision else {
        return ModelState::NoCollision;
    };
    let bad = |why: &str| ModelState::Unreadable(format!("{path}: collision body {why}"));
    let Vertices::Normal(vertices) = &body.vertices else {
        return bad("has no 32-byte vertices");
    };
    let positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.pos).collect();
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        return bad("has a non-finite vertex");
    }
    // `m_nMaxFaceList` triangles read through the index buffer (src:_Common/Object3D.cpp:1727-1760).
    let n_face = usize::try_from(body.counts.n_face).unwrap_or(0);
    let tri_count = n_face.min(body.indices.len() / 3);
    let mut tris = Vec::with_capacity(tri_count);
    for &[a, b, c] in body.indices.as_chunks::<3>().0.iter().take(tri_count) {
        if [a, b, c].iter().any(|&i| usize::from(i) >= positions.len()) {
            return bad("has an index outside its vertex buffer");
        }
        tris.push([a, b, c]);
    }
    if o.bb_min.iter().chain(&o.bb_max).any(|v| !v.is_finite()) {
        return ModelState::Unreadable(format!("{path}: non-finite bounding box"));
    }
    let aabb_min = o.bb_min.map(|v| v.min(EMPTY_BOX));
    let aabb_max = o.bb_max.map(|v| v.max(-EMPTY_BOX));
    ModelState::Mesh(
        Arc::new(CollisionMesh {
            model_index: index,
            positions,
            tris,
            aabb_min,
            aabb_max,
        }),
        instance_flags(&file, elem.trans),
    )
}

/// Builds the `FLYC` chunks of a world folder (Plan/24 §10.7 "Build paths"; WP-WEB-24) with the default
/// [`BuildOptions`]. `folder` is any spelling of the folder name. With `only = Some(lands)` every land's
/// object table is still read (cross-land link cells), but terrain is read and chunks are encoded only for
/// the requested lands (terrain also for their +x/+z/diagonal neighbours, which supply the edge vertices);
/// the returned bytes of a requested land equal the ones of a full build. The chunks are returned in server
/// load order (`z` outer, `x` inner).
#[allow(clippy::type_complexity)]
pub fn build_world_chunks(
    vfs: &Vfs,
    data: &GameData,
    folder: &str,
    only: Option<&[(u16, u16)]>,
) -> Result<(Vec<((u16, u16), Vec<u8>)>, CollisionReport), AssetError> {
    build_world_chunks_with(vfs, data, folder, only, &BuildOptions::default(), &|| false)
}

/// [`build_world_chunks`] with explicit options and a cancellation probe (`cancel() == true` stops the
/// build between files with `ShuttingDown`).
#[allow(clippy::type_complexity)]
pub fn build_world_chunks_with(
    vfs: &Vfs,
    data: &GameData,
    folder: &str,
    only: Option<&[(u16, u16)]>,
    opts: &BuildOptions,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<(Vec<((u16, u16), Vec<u8>)>, CollisionReport), AssetError> {
    let dims = world_dims(data, folder)?;
    let (w, h) = (dims.lands_w, dims.lands_h);
    let n = usize::from(w) * usize::from(h);
    let threads = if opts.threads == 0 {
        auto_threads()
    } else {
        opts.threads
    };
    // Small worlds are not worth threads.
    let threads = if n < 16 { 1 } else { threads };
    let index = |x: u16, z: u16| usize::from(z) * usize::from(w) + usize::from(x);
    let stopped = || AssetError::shutting_down();

    // The lands to encode (load order) and the lands whose terrain is needed.
    let mut wanted: Vec<(u16, u16)> = match only {
        None => (0..h).flat_map(|z| (0..w).map(move |x| (x, z))).collect(),
        Some(list) => {
            for &(x, z) in list {
                if x >= w || z >= h {
                    return Err(AssetError::build(format!(
                        "{}: land {x}-{z} is outside the {w} x {h} world",
                        dims.key
                    )));
                }
            }
            list.to_vec()
        }
    };
    wanted.sort_by_key(|&(x, z)| (z, x));
    wanted.dedup();
    let mut need_terrain = vec![false; n];
    for &(x, z) in &wanted {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (nx, nz) = (x.saturating_add(dx), z.saturating_add(dz));
            if nx < w
                && nz < h
                && let Some(slot) = need_terrain.get_mut(index(nx, nz))
            {
                *slot = true;
            }
        }
    }

    // 1. Every land file: heights (where needed), water, object table.
    let key = dims.key.as_str();
    let mut reads: Vec<LandRead> = par_map(threads, n, &|i| {
        if cancel() {
            return LandRead::missing();
        }
        let (x, z) = (i % usize::from(w), i / usize::from(w));
        // BOUNDS: x < w and z < h, both at most 256
        let (x, z) = (u16::try_from(x).unwrap_or(0), u16::try_from(z).unwrap_or(0));
        read_land(
            vfs,
            &lnd_path(key, x, z),
            need_terrain.get(i).copied().unwrap_or(false),
        )
    });
    if cancel() {
        return Err(stopped());
    }
    let mut report = CollisionReport::default();
    for (i, r) in reads.iter().enumerate() {
        if let Some(why) = &r.failure {
            report.lands_failed += 1;
            let line = format!("land {}-{}: {why}", i % usize::from(w), i / usize::from(w));
            tracing::error!(world = key, "{line}; the land is treated as missing");
            report.fail(line);
        }
    }

    // 2. The models of the OT_OBJ records, once each.
    let indices: BTreeSet<u32> = reads
        .iter()
        .flat_map(|r| r.objects.iter())
        .filter(|o| o.obj_type == OT_OBJ)
        .map(|o| o.model_index)
        .collect();
    let index_list: Vec<u32> = indices.into_iter().collect();
    let loaded = par_map(threads, index_list.len(), &|k| {
        if cancel() {
            return ModelState::Unreadable("cancelled".to_owned());
        }
        match index_list.get(k) {
            Some(&m) => load_model(vfs, data, m),
            None => ModelState::Unknown,
        }
    });
    if cancel() {
        return Err(stopped());
    }
    let mut models: BTreeMap<u32, ModelState> = BTreeMap::new();
    for (m, state) in index_list.iter().zip(loaded) {
        match &state {
            ModelState::Mesh(..) => report.models_loaded += 1,
            ModelState::Unreadable(why) => {
                report.models_failed += 1;
                tracing::error!(world = key, model = *m, "{why}; its objects are dropped");
                report.fail(format!("model {m}: {why}"));
            }
            _ => {}
        }
        models.insert(*m, state);
    }

    // 3. The assembly inputs, every land in load order.
    let mut lands = Vec::with_capacity(n);
    for (i, read) in reads.iter_mut().enumerate() {
        let (x, z) = (i % usize::from(w), i / usize::from(w));
        // BOUNDS: see above
        let (x, z) = (u16::try_from(x).unwrap_or(0), u16::try_from(z).unwrap_or(0));
        let mut objects = Vec::with_capacity(read.objects.len());
        for o in read.objects.drain(..) {
            let (mesh, flags) = if o.obj_type == OT_OBJ {
                match models.get(&o.model_index) {
                    Some(ModelState::Mesh(m, f)) => (Some(Arc::clone(m)), *f),
                    Some(ModelState::Unknown) | None => {
                        report.no_mesh_unknown_model += 1;
                        (None, 0)
                    }
                    Some(ModelState::NonMesh) => {
                        report.no_mesh_non_mesh_model += 1;
                        (None, 0)
                    }
                    Some(ModelState::NoCollision) => {
                        report.no_mesh_no_collision_body += 1;
                        (None, 0)
                    }
                    Some(ModelState::Unreadable(_)) => {
                        report.no_mesh_unreadable_model += 1;
                        (None, 0)
                    }
                }
            } else {
                (None, 0)
            };
            objects.push(ObjectInput {
                ordinal: o.ordinal,
                obj_type: o.obj_type,
                model_index: o.model_index,
                pos_file: o.pos_file,
                angles: o.angles,
                scale: o.scale,
                mesh,
                flags,
            });
        }
        lands.push(LandInput {
            x,
            z,
            present: read.present,
            heights_raw: read.heights.take(),
            water: read.water,
            objects,
        });
    }
    let inputs = CollisionInputs {
        folder: dims.key.clone(),
        mpu: dims.mpu,
        lands_w: w,
        lands_h: h,
        lands,
    };

    // 4. The object pass over every land, then terrain + encoding of the wanted lands.
    let asm = WorldAssembly::new(&inputs, &mut report);
    let parts = par_map(threads, wanted.len(), &|k| {
        let mut rep = CollisionReport::default();
        let Some(&(x, z)) = wanted.get(k) else {
            return (None, rep);
        };
        if cancel() {
            return (None, rep);
        }
        let Some(chunk) = asm.chunk(x, z, &mut rep) else {
            return (None, rep);
        };
        let bytes = encode_chunk(&chunk);
        rep.bytes += bytes.len() as u64;
        (Some(((x, z), bytes)), rep)
    });
    if cancel() {
        return Err(stopped());
    }
    let mut out = Vec::with_capacity(wanted.len());
    for (item, rep) in parts {
        report.merge(&rep);
        out.extend(item);
    }
    Ok((out, report))
}
