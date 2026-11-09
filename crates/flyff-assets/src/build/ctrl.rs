//! Ctrl collision models `FLYK` v1 (kind `ctrl-models`, Plan/24 §10.8, Plan/31 §10.10; WP-WEB-19): the
//! LOD-0 rigid objects of every `mdlDyna.inc` ctrl model, as one file both sides load.
//!
//! The dynamic `OT_CTRL` collision of the original tests the *LOD 0 render mesh* of a ctrl transformed by
//! each rigid object's animated update matrix (`mTM = _mUpdate[i] · mWorld`, `Object3D.cpp:1712-1735`). This
//! builder extracts exactly that from the `.o3d` (`flyff_formats::o3d::parse`, full geometry):
//!
//! * **models** = every `ctrl` entry of `mdlDyna.inc` of model type `MODELTYPE_MESH` / `ANIMATED_MESH`
//!   (billboards such as `rain`/`snow` have no mesh); the file is `model/ctrl_<name>.o3d`
//!   (`ModelRegistry::model_file`). A model whose `.o3d` is missing, unparsable or has a bad header name is
//!   reported and absent from the set (its ctrls get no collider at run time, as in the original, which
//!   could not load them either);
//! * **objects** = the LOD group 0 `GMOBJECT`s in file order, vertex positions object-local and the triangle
//!   list of the body as stored (ref/18 §4.2-§4.3); parents precede children;
//! * **skinned objects are skipped** (DROP, model flag bit 0: the original logs an error and reads them with
//!   the wrong stride); a child of a skipped object becomes a root (counted as `orphaned`);
//! * **bone-parented objects are frozen** (`parentType == GMT_BONE`, model flag bit 1, `kind 2`): they keep
//!   no node keys and `local_tm` is pre-multiplied by the bind-pose bone matrix `baseBone[parentIdx]` of the
//!   embedded skeleton (the same rule as `flyff_formats::pack::motion::build_nodes_from_o3d`); a bone of an
//!   external `.chr` (no embedded skeleton) leaves `local_tm` alone. Needs a ledger row (Plan/24 §22 Q4);
//! * **node animation**: an object of a file with `maxFrame > 0` that stores node keys keeps all `maxFrame`
//!   `(rotation, position)` keys (its `localTM` is ignored while animating, ref/18 §4.8);
//! * `per_slerp_raw` is the **raw header value** (`trunc3` is applied once by the sim's clock, Plan/31
//!   §12.8), `attrs` the flagged entries of the built-in attribute array, `aabb` the header box.
//!
//! The writer ([`write_ctrl_models`]) emits the canonical layout of Plan/24 §10.8; the only reader is
//! `flyff_sim::decode_ctrl_models`.

use crate::cache::Cache;
use crate::derived::ServerDerived;
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{
    AssetBuilder, AssetKind, BuildCtx, BuiltFile, Compress, JobKey, KeyRoute, NO_PARTITION,
};
use crate::prep::AssetPrep;
use crate::util::lock;
use flyff_data::models::ModelRegistry;
use flyff_formats::error::{ErrorKind, WarningCode};
use flyff_formats::o3d::{self, AttrBlock, GmType, O3dFile, O3dOptions, Vertices};
use flyff_sim::anim::motion_table::MotionAttr;
use flyff_sim::collision::chunk::CollisionMesh;
use flyff_sim::collision::ctrl_models::{
    CTRL_MODELS_FORMAT_VERSION, CtrlModel, CtrlModelSet, MODEL_BONE_PARENTED_FROZEN,
    MODEL_NO_LOD0_GEOMETRY, MODEL_SKINNED_SKIPPED, OBJECT_KIND_FROZEN, OBJECT_KIND_NORMAL,
    RigidObject, decode_ctrl_models,
};
use flyff_sim::collision::geom::Aabb;
use flyff_sim::num::Vec3;
use flyff_sim::pose::quat::Quat;
use flyff_vfs::Vfs;
use std::sync::{Arc, Mutex};

/// Bump on any change that alters the produced bytes (model selection, object rules, ordering).
pub const BUILDER_VERSION: u32 = 1;
/// The byte layout version (`FLYK` v1).
pub const FORMAT_VERSION: u32 = CTRL_MODELS_FORMAT_VERSION as u32;
/// The key of the model set.
pub const KEY_BIN: &str = "all.bin";
/// The key of the build report (diagnostics, not referenced by the client manifest).
pub const KEY_REPORT: &str = "report.json";
/// The compression level of the stored artifact (the file is small, Plan/24 §15.1: < 5 MB).
pub const GZIP_LEVEL: u32 = 6;

/// `OT_CTRL`.
const OT_CTRL: u32 = 2;
/// `MODELTYPE_MESH`.
const MODELTYPE_MESH: u32 = 1;
/// `MODELTYPE_ANIMATED_MESH`.
const MODELTYPE_ANIMATED_MESH: u32 = 2;

/// What the build saw (logged by the builder, asserted by the corpus test; `report.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CtrlReport {
    /// `mdlDyna.inc` ctrl entries of a mesh model type (the candidates).
    pub candidates: usize,
    /// Models in the file.
    pub models: usize,
    /// Rigid objects written.
    pub objects: usize,
    /// Triangles written.
    pub triangles: usize,
    /// Models with at least one node-animated object.
    pub animated_models: usize,
    /// Models that had skinned objects (skipped, flag bit 0).
    pub models_with_skinned: usize,
    /// Skinned objects skipped.
    pub skinned_objects: usize,
    /// Models with bone-parented objects (frozen, flag bit 1).
    pub models_with_frozen: usize,
    /// Bone-parented objects frozen.
    pub frozen_objects: usize,
    /// Objects whose parent was a skipped object (they became roots).
    pub orphaned: usize,
    /// Models without any LOD-0 object (flag bit 2).
    pub no_lod0_geometry: Vec<String>,
    /// `(ctrl_index, path)` of candidates whose `.o3d` does not exist.
    pub missing: Vec<(u32, String)>,
    /// `(path, message)` of candidates whose `.o3d` failed to parse or validate.
    pub failed: Vec<(String, String)>,
    /// Files with a header-name mismatch (loaded anyway: ref/18 §4.10 "load and warn").
    pub name_mismatch: Vec<String>,
}

/// The value of one build.
#[derive(Clone, Debug)]
pub struct CtrlBuild {
    /// The models.
    pub set: CtrlModelSet,
    /// What was skipped or noted.
    pub report: CtrlReport,
}

/// Counters of the objects of one converted model.
#[derive(Clone, Copy, Debug, Default)]
struct ObjectCounts {
    skinned: usize,
    frozen: usize,
    orphaned: usize,
}

fn model_path(file: &str) -> Option<String> {
    Vfs::normalize(&format!("model/{file}"))
        .ok()
        .map(|p| p.as_str().to_owned())
}

fn v3(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

/// Converts a parsed `.o3d` into the `FLYK` model of ctrl `ctrl_index` (the rules in the module docs).
/// Fails with a message when the file cannot be used (non-finite values, bad indices, an inconsistent key
/// count).
fn model_from_o3d(
    ctrl_index: u32,
    path: &str,
    f: &O3dFile,
) -> Result<(CtrlModel, ObjectCounts), String> {
    let max_frame = u32::try_from(f.max_frame).map_err(|_| "negative maxFrame".to_owned())?;
    if !f.per_slerp.is_finite() {
        return Err("non-finite perSlerp".to_owned());
    }
    let finite3 = |v: [f32; 3]| v.iter().all(|x| x.is_finite());
    if !finite3(f.bb_min) || !finite3(f.bb_max) {
        return Err("non-finite bounding box".to_owned());
    }
    let mut counts = ObjectCounts::default();
    let mut flags = 0u32;
    let mut objects: Vec<RigidObject> = Vec::new();
    // original object index -> index in `objects` (None = skipped)
    let mut index_map: Vec<Option<usize>> = Vec::new();
    match f.lod0() {
        None => flags |= MODEL_NO_LOD0_GEOMETRY,
        Some(group) if group.objects.is_empty() => flags |= MODEL_NO_LOD0_GEOMETRY,
        Some(group) => {
            for (oi, obj) in group.objects.iter().enumerate() {
                if matches!(obj.kind(), GmType::Skin) {
                    counts.skinned += 1;
                    flags |= MODEL_SKINNED_SKIPPED;
                    index_map.push(None);
                    continue;
                }
                let Vertices::Normal(verts) = &obj.body.vertices else {
                    return Err(format!("object {oi}: no rigid vertex buffer"));
                };
                if verts.len() > 65_536 {
                    return Err(format!("object {oi}: more than 65536 vertices"));
                }
                let positions: Vec<[f32; 3]> = verts.iter().map(|v| v.pos).collect();
                if positions.iter().flatten().any(|x| !x.is_finite()) {
                    return Err(format!("object {oi}: non-finite vertex"));
                }
                let mut tris: Vec<[u16; 3]> = Vec::with_capacity(obj.body.indices.len() / 3);
                for tri in obj.body.indices.as_chunks::<3>().0.iter().copied() {
                    if tri.iter().any(|&i| usize::from(i) >= positions.len()) {
                        return Err(format!("object {oi}: triangle index out of range"));
                    }
                    tris.push(tri);
                }
                let bone_parented = obj.parent_idx != -1 && obj.parent_type == Some(2);
                let (parent, frozen, local_tm) = if bone_parented {
                    counts.frozen += 1;
                    flags |= MODEL_BONE_PARENTED_FROZEN;
                    let base = f.bones.as_ref().and_then(|b| {
                        usize::try_from(obj.parent_idx)
                            .ok()
                            .and_then(|p| b.base.get(p))
                    });
                    let tm = match base {
                        Some(m) => obj.local_tm.mul(m),
                        None => obj.local_tm,
                    };
                    (-1, true, tm)
                } else if obj.parent_idx == -1 {
                    (-1, false, obj.local_tm)
                } else {
                    let p = usize::try_from(obj.parent_idx)
                        .ok()
                        .filter(|p| *p < oi)
                        .ok_or_else(|| {
                            format!("object {oi}: parent does not precede the object")
                        })?;
                    match index_map.get(p).copied().flatten() {
                        Some(mapped) => (
                            i32::try_from(mapped).map_err(|_| "object count".to_owned())?,
                            false,
                            obj.local_tm,
                        ),
                        None => {
                            counts.orphaned += 1;
                            (-1, false, obj.local_tm)
                        }
                    }
                };
                if local_tm.0.iter().any(|x| !x.is_finite()) {
                    return Err(format!("object {oi}: non-finite localTM"));
                }
                let frames: Vec<(Quat, Vec3)> = match (&obj.node_anim, frozen) {
                    (Some(n), false) if max_frame > 0 && n.has_frames != 0 => {
                        if n.keys.len() != max_frame as usize {
                            return Err(format!(
                                "object {oi}: {} keys for {max_frame} frames",
                                n.keys.len()
                            ));
                        }
                        let mut keys = Vec::with_capacity(n.keys.len());
                        for k in &n.keys {
                            let q = k.rot;
                            let p = k.pos;
                            if q.iter().chain(p.iter()).any(|x| !x.is_finite()) {
                                return Err(format!("object {oi}: non-finite node key"));
                            }
                            keys.push((Quat::new(q[0], q[1], q[2], q[3]), v3(p)));
                        }
                        keys
                    }
                    _ => Vec::new(),
                };
                index_map.push(Some(objects.len()));
                objects.push(RigidObject {
                    parent,
                    frozen,
                    local_tm,
                    mesh: CollisionMesh {
                        positions,
                        tris,
                        ..CollisionMesh::default()
                    },
                    frames,
                });
            }
            if objects.is_empty() {
                flags |= MODEL_NO_LOD0_GEOMETRY;
            }
        }
    }
    let mut attrs = Vec::new();
    if let AttrBlock::Attrs(list) = &f.attrs {
        for (i, a) in list.iter().enumerate() {
            if a.flags != 0 {
                if !a.frame.is_finite() {
                    return Err(format!("attribute {i}: non-finite frame"));
                }
                attrs.push(MotionAttr {
                    index: u32::try_from(i).map_err(|_| "attribute index".to_owned())?,
                    flags: a.flags,
                    sound: a.sound_id,
                    frame: a.frame,
                });
            }
        }
    }
    let model = CtrlModel {
        ctrl_index,
        path: path.to_owned(),
        objects,
        max_frame,
        per_slerp_raw: f.per_slerp,
        attrs,
        flags,
        aabb: Aabb {
            min: v3(f.bb_min),
            max: v3(f.bb_max),
        },
    };
    Ok((model, counts))
}

/// The mesh-type `ctrl` entries of `mdlDyna.inc` as `(ctrl_index, VFS path)`, ascending index.
pub fn candidates(registry: &ModelRegistry) -> Vec<(u32, String)> {
    let Some(table) = registry.by_type.get(&OT_CTRL) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (_, elem) in table.iter() {
        if elem.model_type != MODELTYPE_MESH && elem.model_type != MODELTYPE_ANIMATED_MESH {
            continue;
        }
        let Some(path) = registry
            .model_file(OT_CTRL, elem.index)
            .and_then(|file| model_path(&file))
        else {
            continue;
        };
        out.push((elem.index, path));
    }
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Builds the model set from the registry and the `.o3d` files (a pure function of the VFS bytes and the
/// registry). `cancelled` is polled between files; a cancelled build is `ShuttingDown`.
pub fn build_ctrl_models(
    vfs: &Vfs,
    registry: &ModelRegistry,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<CtrlBuild, AssetError> {
    let cands = candidates(registry);
    let mut report = CtrlReport {
        candidates: cands.len(),
        ..CtrlReport::default()
    };
    let mut set = CtrlModelSet::default();
    for (index, path) in cands {
        if cancelled() {
            return Err(AssetError::shutting_down());
        }
        let bytes = match vfs.read(&path) {
            Ok(b) => b,
            Err(_) => {
                report.missing.push((index, path));
                continue;
            }
        };
        let file_name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let parsed = match o3d::parse(&bytes, &O3dOptions::full().with_file_name(&file_name)) {
            Ok(p) => p,
            Err(e) if e.kind == ErrorKind::LegacyLayout => {
                report.failed.push((path, "pre-v20 layout".to_owned()));
                continue;
            }
            Err(e) => {
                report.failed.push((path, e.to_string()));
                continue;
            }
        };
        if parsed
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::O3dNameMismatch)
        {
            report.name_mismatch.push(path.clone());
        }
        match model_from_o3d(index, &path, &parsed.value) {
            Ok((model, c)) => {
                report.objects += model.objects.len();
                report.triangles += model
                    .objects
                    .iter()
                    .map(|o| o.mesh.tris.len())
                    .sum::<usize>();
                report.animated_models += usize::from(model.animated());
                report.skinned_objects += c.skinned;
                report.models_with_skinned += usize::from(model.flags & MODEL_SKINNED_SKIPPED != 0);
                report.frozen_objects += c.frozen;
                report.models_with_frozen +=
                    usize::from(model.flags & MODEL_BONE_PARENTED_FROZEN != 0);
                report.orphaned += c.orphaned;
                if model.flags & MODEL_NO_LOD0_GEOMETRY != 0 {
                    report.no_lod0_geometry.push(path.clone());
                }
                set.models.insert(index, model);
            }
            Err(msg) => report.failed.push((path, msg)),
        }
    }
    report.models = set.models.len();
    Ok(CtrlBuild { set, report })
}

// ---------------------------------------------------------------------------------------------------
// the writer (Plan/24 §10.8)
// ---------------------------------------------------------------------------------------------------

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn len32(n: usize) -> Result<u32, AssetError> {
    u32::try_from(n).map_err(|_| AssetError::build("FLYK: a table exceeds 4 GiB"))
}

/// Serialises `set` as a `FLYK` v1 file (Plan/24 §10.8): models in ascending `ctrl_index`, objects and
/// attributes consecutive, each object's positions / triangles (block padded to 4) / keys tiling the data area
/// in order, the UTF-8 paths consecutive after it, zero padding to a multiple of 4. The result is byte-for-byte
/// what `flyff_sim::testkit::ctrl_models::encode_ctrl_models` produces for the same set.
pub fn write_ctrl_models(set: &CtrlModelSet) -> Result<Vec<u8>, AssetError> {
    let mut models = Vec::new();
    let mut objects = Vec::new();
    let mut attrs = Vec::new();
    let mut data = Vec::new();
    let mut strings = Vec::new();
    let (mut n_obj, mut n_attr) = (0usize, 0usize);
    for m in set.models.values() {
        put_u32(&mut models, m.ctrl_index);
        put_u32(&mut models, len32(n_obj)?);
        put_u32(&mut models, len32(m.objects.len())?);
        put_u32(&mut models, m.max_frame);
        put_f32(&mut models, m.per_slerp_raw);
        put_u32(&mut models, len32(n_attr)?);
        put_u32(&mut models, len32(m.attrs.len())?);
        put_u32(&mut models, m.flags);
        for v in [m.aabb.min, m.aabb.max] {
            put_f32(&mut models, v.x);
            put_f32(&mut models, v.y);
            put_f32(&mut models, v.z);
        }
        put_u32(&mut models, len32(strings.len())?);
        put_u32(&mut models, len32(m.path.len())?);
        strings.extend_from_slice(m.path.as_bytes());
        n_obj += m.objects.len();
        n_attr += m.attrs.len();
        for o in &m.objects {
            put_i32(&mut objects, o.parent);
            put_u32(
                &mut objects,
                if o.frozen {
                    OBJECT_KIND_FROZEN
                } else {
                    OBJECT_KIND_NORMAL
                },
            );
            for v in o.local_tm.0 {
                put_f32(&mut objects, v);
            }
            put_u32(&mut objects, len32(o.mesh.positions.len())?);
            put_u32(&mut objects, len32(o.mesh.tris.len())?);
            put_u32(&mut objects, len32(data.len())?);
            for p in &o.mesh.positions {
                for c in p {
                    put_f32(&mut data, *c);
                }
            }
            put_u32(&mut objects, len32(data.len())?);
            for t in &o.mesh.tris {
                for i in t {
                    put_u16(&mut data, *i);
                }
            }
            while !data.len().is_multiple_of(4) {
                data.push(0);
            }
            put_u32(&mut objects, len32(o.frames.len())?);
            put_u32(&mut objects, len32(data.len())?);
            for (q, p) in &o.frames {
                for v in [q.x, q.y, q.z, q.w, p.x, p.y, p.z] {
                    put_f32(&mut data, v);
                }
            }
        }
        for a in &m.attrs {
            put_u32(&mut attrs, a.index);
            put_u32(&mut attrs, a.flags);
            put_i32(&mut attrs, a.sound);
            put_f32(&mut attrs, a.frame);
        }
    }
    while !strings.len().is_multiple_of(4) {
        strings.push(0);
    }
    let data_offset = 32 + models.len() + objects.len() + attrs.len();
    let strings_offset = data_offset + data.len();
    let file_len = strings_offset + strings.len();
    let mut out = Vec::with_capacity(file_len);
    out.extend_from_slice(b"FLYK");
    put_u16(&mut out, CTRL_MODELS_FORMAT_VERSION);
    put_u16(&mut out, 0);
    put_u32(&mut out, len32(file_len)?);
    put_u32(&mut out, len32(set.models.len())?);
    put_u32(&mut out, len32(n_obj)?);
    put_u32(&mut out, len32(n_attr)?);
    put_u32(&mut out, len32(data_offset)?);
    put_u32(&mut out, len32(strings_offset)?);
    out.extend_from_slice(&models);
    out.extend_from_slice(&objects);
    out.extend_from_slice(&attrs);
    out.extend_from_slice(&data);
    out.extend_from_slice(&strings);
    Ok(out)
}

/// The `report.json` bytes of a build (compact, no timings).
pub fn report_json(r: &CtrlReport) -> Vec<u8> {
    let list = |v: &[String]| -> String {
        let items: Vec<String> = v.iter().map(|s| format!("{s:?}")).collect();
        format!("[{}]", items.join(","))
    };
    let missing: Vec<String> = r
        .missing
        .iter()
        .map(|(i, p)| format!("[{i},{p:?}]"))
        .collect();
    let failed: Vec<String> = r
        .failed
        .iter()
        .map(|(p, e)| format!("[{p:?},{e:?}]"))
        .collect();
    format!(
        "{{\"candidates\":{},\"models\":{},\"objects\":{},\"triangles\":{},\"animated_models\":{},\
\"models_with_skinned\":{},\"skinned_objects\":{},\"models_with_frozen\":{},\"frozen_objects\":{},\
\"orphaned\":{},\"no_lod0_geometry\":{},\"missing\":[{}],\"failed\":[{}],\"name_mismatch\":{}}}",
        r.candidates,
        r.models,
        r.objects,
        r.triangles,
        r.animated_models,
        r.models_with_skinned,
        r.skinned_objects,
        r.models_with_frozen,
        r.frozen_objects,
        r.orphaned,
        list(&r.no_lod0_geometry),
        missing.join(","),
        failed.join(","),
        list(&r.name_mismatch),
    )
    .into_bytes()
}

// ---------------------------------------------------------------------------------------------------
// the builder
// ---------------------------------------------------------------------------------------------------

/// Fingerprint inputs shared by every call, memoised per VFS (a `set_fp` of the 4 198 `.o3d` costs ~0.5 s).
#[derive(Default)]
struct FpCache {
    vfs: (usize, usize),
    model: Option<Fp>,
    registry: Option<Fp>,
}

/// The builder of kind [`AssetKind::CtrlModels`]: one job, no partitions.
#[derive(Default)]
pub struct CtrlBuilder {
    cache: Mutex<FpCache>,
}

impl CtrlBuilder {
    /// A builder.
    pub fn new() -> CtrlBuilder {
        CtrlBuilder::default()
    }
}

impl AssetBuilder for CtrlBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::CtrlModels
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        FORMAT_VERSION
    }

    /// `all.bin` or `report.json`.
    fn route(&self, _ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        if key != KEY_BIN && key != KEY_REPORT {
            return Err(AssetError::bad_key(format!(
                "{key:?}: ctrl-models has the keys {KEY_BIN:?} and {KEY_REPORT:?}"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::single(AssetKind::CtrlModels, ""),
            partition: NO_PARTITION.to_owned(),
        })
    }

    /// `set_fp("model", ["o3d"])`, `registry_fp` (Plan/24 §6.3).
    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        if partition != NO_PARTITION {
            return None;
        }
        let mut c = lock(&self.cache);
        let id = (std::ptr::from_ref(ctx.vfs) as usize, ctx.vfs.len());
        if c.vfs != id {
            *c = FpCache {
                vfs: id,
                ..FpCache::default()
            };
        }
        let model = *c.model.get_or_insert_with(|| ctx.set_fp("model", &["o3d"]));
        let registry = *c.registry.get_or_insert_with(|| ctx.registry_fp());
        Some(ctx.kind_fp(
            AssetKind::CtrlModels,
            BUILDER_VERSION,
            FORMAT_VERSION,
            &[model, registry],
        ))
    }

    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        vec![JobKey::single(AssetKind::CtrlModels, "")]
    }

    fn build(&self, ctx: &BuildCtx<'_>, _job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let built = build_ctrl_models(ctx.vfs, &ctx.data.models, &|| ctx.cancelled())?;
        for (idx, path) in &built.report.missing {
            tracing::warn!(ctrl = idx, %path, "ctrl-models: the .o3d of a mdlDyna ctrl does not exist");
        }
        for (path, err) in &built.report.failed {
            tracing::warn!(%path, error = %err, "ctrl-models: unusable .o3d skipped");
        }
        for path in &built.report.name_mismatch {
            tracing::warn!(%path, "ctrl-models: the .o3d header name differs from the file name");
        }
        let bin = write_ctrl_models(&built.set)?;
        let compress = Compress::IfBeneficial { level: GZIP_LEVEL };
        Ok(vec![
            BuiltFile {
                key: KEY_BIN.to_owned(),
                bytes: bin,
                content_type: "application/octet-stream",
                compress,
            },
            BuiltFile {
                key: KEY_REPORT.to_owned(),
                bytes: report_json(&built.report),
                content_type: "application/json",
                compress,
            },
        ])
    }

    fn content_type(&self, key: &str) -> &'static str {
        if key == KEY_REPORT {
            "application/json"
        } else {
            "application/octet-stream"
        }
    }

    fn on_start(&self, _ctx: &BuildCtx<'_>, _cache: &Cache) -> Result<(), AssetError> {
        Ok(())
    }

    /// `decode_ctrl_models(all.bin)` into `ServerDerived.ctrl_models`.
    fn load_server(&self, prep: &AssetPrep, out: &mut ServerDerived) -> Result<(), AssetError> {
        let file = prep
            .lookup(AssetKind::CtrlModels, KEY_BIN)?
            .ok_or_else(|| AssetError::build("ctrl-models/all.bin is not in the cache"))?;
        let bytes = file
            .read_decoded()
            .map_err(|e| AssetError::build(format!("ctrl-models/{KEY_BIN}: {e}")))?;
        let set = decode_ctrl_models(&bytes)
            .map_err(|e| AssetError::build(format!("ctrl-models/{KEY_BIN}: {e}")))?;
        tracing::info!(models = set.len(), "ctrl collision models loaded");
        out.ctrl_models = Some(Arc::new(set));
        Ok(())
    }
}
