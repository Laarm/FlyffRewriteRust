//! The `FLYK` v1 ctrl-model decoder (Plan/31 §10.10, Plan/24 §10.8; WP-SIM-28).
//!
//! One file holds the dynamic-collision shape of every `mdlDyna.inc` ctrl model: the LOD-0 rigid objects
//! of the `.o3d` (object-space positions and triangles, `localTM`, node animation keys), the built-in
//! frame attributes, the raw `perSlerp` and the model box. The server loads it from
//! `ServerDerived.ctrl_models`, the browser passes the same bytes once per session to
//! `SimWorld.load_ctrl_models`; this decoder is the only reader (the writer is `flyff-assets`'
//! `build::ctrl`, a test-only encoder lives in `crate::testkit::ctrl_models`).
//!
//! # Layout (little endian, all offsets in bytes)
//!
//! ```text
//! 0   "FLYK"  4 u16 version = 1  6 u16 flags = 0  8 u32 file_len  12 u32 model_count  16 u32 object_count
//! 20  u32 attr_count  24 u32 data_offset  28 u32 strings_offset
//! 32  ModelDesc[model_count]   64 B: ctrl_index, first_object, object_count, max_frame, per_slerp_raw (f32),
//!                                    first_attr, attr_count, flags, aabb_min[3], aabb_max[3], path_offset, path_len
//! ..  ObjectDesc[object_count] 96 B: parent (i32), kind (0 normal, 2 bone-parented/frozen), local_tm[16],
//!                                    vertex_count, triangle_count, vertex_offset, index_offset, frame_count, frames_offset
//! ..  Attr[attr_count]         16 B: index, flags, sound (i32), frame (f32)
//! data_offset: per object in order: positions f32x3, triangles u16x3 (block padded to 4), frames 7 x f32 (qx qy qz qw px py pz)
//! strings_offset: the UTF-8 paths, consecutive, then zero padding to a multiple of 4
//! ```
//!
//! The decoder is strict (the layout is canonical, so `decode(encode(x)) == x` and a re-encoded file is
//! byte-identical): sections must tile the file exactly, models are sorted by unique `ctrl_index`,
//! consecutive object / attribute / path ranges, parents precede their children, indices stay inside the
//! object's vertex list, floats are finite, padding is zero. Every rejection is
//! [`SimError::BadCtrlModels`]; nothing panics or allocates before the counts were checked against the
//! file length.

use super::chunk::CollisionMesh;
use super::geom::Aabb;
use crate::anim::motion_table::MotionAttr;
use crate::num::Vec3;
use crate::pose::quat::{Mat44, Quat};
use crate::types::SimError;
use std::collections::BTreeMap;

/// `FLYK` `format_version` (Plan/24 §10.8).
pub const CTRL_MODELS_FORMAT_VERSION: u16 = 1;

/// Model flag: skinned objects were skipped (DROP: the original read them with the wrong stride).
pub const MODEL_SKINNED_SKIPPED: u32 = 1;
/// Model flag: bone-parented objects are frozen (their `local_tm` carries the bind-pose bone matrix).
pub const MODEL_BONE_PARENTED_FROZEN: u32 = 2;
/// Model flag: the model has no LOD-0 geometry at all (no object).
pub const MODEL_NO_LOD0_GEOMETRY: u32 = 4;
/// All defined model flag bits.
pub const MODEL_FLAGS_MASK: u32 =
    MODEL_SKINNED_SKIPPED | MODEL_BONE_PARENTED_FROZEN | MODEL_NO_LOD0_GEOMETRY;

/// `ObjectDesc.kind`: an ordinary rigid object (animated by its node keys, or `localTM` when it has none).
pub const OBJECT_KIND_NORMAL: u32 = 0;
/// `ObjectDesc.kind`: parented to a bone of the embedded skeleton, frozen at the bind pose.
pub const OBJECT_KIND_FROZEN: u32 = 2;

/// The largest frame count accepted (the `.o3d` parser caps `maxFrame` at 65 535 too).
pub const MAX_FRAME_LIMIT: u32 = 65_535;

const HEADER_LEN: usize = 32;
const MODEL_DESC_LEN: usize = 64;
const OBJECT_DESC_LEN: usize = 96;
const ATTR_LEN: usize = 16;
const KEY_LEN: usize = 28;

/// All ctrl models of a file by `mdlDyna` ctrl index (the `CtrlModelKey`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtrlModelSet {
    /// The models in ascending `ctrl_index` order.
    pub models: BTreeMap<u32, CtrlModel>,
}

/// One ctrl model (`ModelDesc` plus its objects and attributes).
#[derive(Clone, Debug, PartialEq)]
pub struct CtrlModel {
    /// The `mdlDyna.inc` ctrl index (`CI_*`, equals the `MotionTable` `(OT_CTRL, index)` key).
    pub ctrl_index: u32,
    /// Lower-case VFS path of the `.o3d`.
    pub path: String,
    /// The LOD-0 rigid objects in file order (parents precede children).
    pub objects: Vec<RigidObject>,
    /// Frames of the built-in animation (`0` = static).
    pub max_frame: u32,
    /// The raw `perSlerp` of the `.o3d` header; the clock applies `trunc3` exactly once (Plan/31 §12.8).
    pub per_slerp_raw: f32,
    /// The built-in frame attributes (flagged entries only, ascending `index`).
    pub attrs: Vec<MotionAttr>,
    /// `MODEL_*` flag bits.
    pub flags: u32,
    /// The model box (`bbMin`/`bbMax` of the header): the OBB source of the collider.
    pub aabb: Aabb,
}

/// One rigid LOD-0 object.
#[derive(Clone, Debug, PartialEq)]
pub struct RigidObject {
    /// `-1` for a root, else the index of an earlier object of the same model.
    pub parent: i32,
    /// Bone-parented and frozen (`FLYK` kind 2): `local_tm` already includes the bind-pose bone matrix.
    pub frozen: bool,
    /// `localTM`, used when the object has no node keys (ignored while animating, ref/18 §4.8).
    pub local_tm: Mat44,
    /// The object-space triangles (`mesh.positions`, `mesh.tris` in file order; the other fields of
    /// [`CollisionMesh`] are unused here and stay at their defaults).
    pub mesh: CollisionMesh,
    /// Node keys `(rotation, position)`: empty, or exactly `max_frame` entries.
    pub frames: Vec<(Quat, Vec3)>,
}

impl RigidObject {
    /// `true` when the object has node keys.
    pub fn animated(&self) -> bool {
        !self.frames.is_empty()
    }
}

impl CtrlModel {
    /// `true` when any object has node keys (the model animates).
    pub fn animated(&self) -> bool {
        self.max_frame > 0 && self.objects.iter().any(RigidObject::animated)
    }

    /// Approximate heap bytes (for `memory_report`).
    pub fn heap_bytes(&self) -> usize {
        self.path.len()
            + self.attrs.len() * size_of::<MotionAttr>()
            + self
                .objects
                .iter()
                .map(|o| {
                    o.mesh.positions.len() * size_of::<[f32; 3]>()
                        + o.mesh.tris.len() * size_of::<[u16; 3]>()
                        + o.frames.len() * size_of::<(Quat, Vec3)>()
                        + size_of::<RigidObject>()
                })
                .sum::<usize>()
    }
}

impl CtrlModelSet {
    /// The model of ctrl index `ctrl_index`.
    pub fn get(&self, ctrl_index: u32) -> Option<&CtrlModel> {
        self.models.get(&ctrl_index)
    }

    /// Number of models.
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// `true` when the set has no model.
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Approximate heap bytes of all models.
    pub fn heap_bytes(&self) -> usize {
        self.models.values().map(CtrlModel::heap_bytes).sum()
    }

    /// Decodes a `FLYK` v1 file ([`decode_ctrl_models`]).
    pub fn decode(bytes: &[u8]) -> Result<CtrlModelSet, SimError> {
        decode_ctrl_models(bytes)
    }
}

fn bad(why: &'static str) -> SimError {
    SimError::BadCtrlModels(why)
}

/// A checked little-endian cursor over the file.
struct Rd<'a> {
    b: &'a [u8],
}

impl<'a> Rd<'a> {
    fn at(&self, off: usize, len: usize) -> Result<&'a [u8], SimError> {
        let end = off.checked_add(len).ok_or_else(|| bad("offset overflow"))?;
        self.b.get(off..end).ok_or_else(|| bad("truncated"))
    }

    fn u16(&self, off: usize) -> Result<u16, SimError> {
        let s = self.at(off, 2)?;
        Ok(u16::from_le_bytes(
            s.try_into().map_err(|_| bad("truncated"))?,
        ))
    }

    fn u32(&self, off: usize) -> Result<u32, SimError> {
        let s = self.at(off, 4)?;
        Ok(u32::from_le_bytes(
            s.try_into().map_err(|_| bad("truncated"))?,
        ))
    }

    fn i32(&self, off: usize) -> Result<i32, SimError> {
        let s = self.at(off, 4)?;
        Ok(i32::from_le_bytes(
            s.try_into().map_err(|_| bad("truncated"))?,
        ))
    }

    /// A finite `f32`.
    fn f32(&self, off: usize) -> Result<f32, SimError> {
        let s = self.at(off, 4)?;
        let v = f32::from_le_bytes(s.try_into().map_err(|_| bad("truncated"))?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(bad("non-finite float"))
        }
    }

    fn vec3(&self, off: usize) -> Result<Vec3, SimError> {
        Ok(Vec3::new(
            self.f32(off)?,
            self.f32(off + 4)?,
            self.f32(off + 8)?,
        ))
    }
}

/// `usize` of a `u32` field.
fn us(v: u32) -> usize {
    v as usize // BOUNDS: usize is at least 32 bits on every supported target
}

/// Rounds up to a multiple of 4.
fn align4(n: usize) -> Result<usize, SimError> {
    n.checked_add(3)
        .map(|v| v & !3)
        .ok_or_else(|| bad("offset overflow"))
}

struct ModelHead {
    ctrl_index: u32,
    first_object: usize,
    object_count: usize,
    max_frame: u32,
    per_slerp_raw: f32,
    first_attr: usize,
    attr_count: usize,
    flags: u32,
    aabb: Aabb,
    path_offset: usize,
    path_len: usize,
}

/// Decodes a `FLYK` v1 file into a [`CtrlModelSet`].
///
/// Rejects (all as [`SimError::BadCtrlModels`]): a wrong magic, version or flags, a `file_len` that is not the
/// file length, header counts that cannot fit the file, section offsets that do not tile the file, unsorted
/// or duplicate `ctrl_index`, non-consecutive object / attribute / path ranges, a parent that does not precede
/// its child, a bad object `kind`, a frozen object with node keys, a key count other than `0` / `max_frame`,
/// triangle indices outside the vertex list, unordered or out-of-range attributes, non-UTF-8 paths, unknown
/// model flag bits, non-finite floats and non-zero padding. Never panics.
pub fn decode_ctrl_models(bytes: &[u8]) -> Result<CtrlModelSet, SimError> {
    let rd = Rd { b: bytes };
    if bytes.get(..4) != Some(b"FLYK") {
        return Err(bad("wrong magic"));
    }
    if rd.u16(4)? != CTRL_MODELS_FORMAT_VERSION || rd.u16(6)? != 0 {
        return Err(bad("unsupported version or flags"));
    }
    if us(rd.u32(8)?) != bytes.len() {
        return Err(bad("file_len differs from the file length"));
    }
    let model_count = us(rd.u32(12)?);
    let object_count = us(rd.u32(16)?);
    let attr_count = us(rd.u32(20)?);
    let data_offset = us(rd.u32(24)?);
    let strings_offset = us(rd.u32(28)?);
    // the counted tables must fit the file before anything is allocated
    let table_end = model_count
        .checked_mul(MODEL_DESC_LEN)
        .and_then(|m| object_count.checked_mul(OBJECT_DESC_LEN).map(|o| (m, o)))
        .and_then(|(m, o)| attr_count.checked_mul(ATTR_LEN).map(|a| (m, o, a)))
        .and_then(|(m, o, a)| HEADER_LEN.checked_add(m)?.checked_add(o)?.checked_add(a))
        .ok_or_else(|| bad("table sizes overflow"))?;
    if table_end > bytes.len() {
        return Err(bad("tables exceed the file"));
    }
    if data_offset != table_end {
        return Err(bad("data_offset is not the end of the tables"));
    }
    if strings_offset < data_offset || strings_offset > bytes.len() {
        return Err(bad("strings_offset out of range"));
    }
    let models_off = HEADER_LEN;
    let objects_off = models_off + model_count * MODEL_DESC_LEN;
    let attrs_off = objects_off + object_count * OBJECT_DESC_LEN;

    // pass 1: the model heads (ranges are checked against the running cursors)
    let mut heads: Vec<ModelHead> = Vec::with_capacity(model_count);
    let (mut next_obj, mut next_attr, mut next_path) = (0usize, 0usize, 0usize);
    let strings_len = bytes.len() - strings_offset;
    for i in 0..model_count {
        let o = models_off + i * MODEL_DESC_LEN;
        let head = ModelHead {
            ctrl_index: rd.u32(o)?,
            first_object: us(rd.u32(o + 4)?),
            object_count: us(rd.u32(o + 8)?),
            max_frame: rd.u32(o + 12)?,
            per_slerp_raw: rd.f32(o + 16)?,
            first_attr: us(rd.u32(o + 20)?),
            attr_count: us(rd.u32(o + 24)?),
            flags: rd.u32(o + 28)?,
            aabb: Aabb {
                min: rd.vec3(o + 32)?,
                max: rd.vec3(o + 44)?,
            },
            path_offset: us(rd.u32(o + 56)?),
            path_len: us(rd.u32(o + 60)?),
        };
        if heads
            .last()
            .is_some_and(|p: &ModelHead| p.ctrl_index >= head.ctrl_index)
        {
            return Err(bad("models are not sorted by unique ctrl_index"));
        }
        if head.flags & !MODEL_FLAGS_MASK != 0 {
            return Err(bad("unknown model flag bits"));
        }
        if head.max_frame > MAX_FRAME_LIMIT {
            return Err(bad("max_frame out of range"));
        }
        if head.first_object != next_obj || head.first_attr != next_attr {
            return Err(bad("object or attribute ranges are not consecutive"));
        }
        if head.path_offset != next_path || head.path_len == 0 {
            return Err(bad("paths are not consecutive"));
        }
        next_obj = next_obj
            .checked_add(head.object_count)
            .ok_or_else(|| bad("object range overflow"))?;
        next_attr = next_attr
            .checked_add(head.attr_count)
            .ok_or_else(|| bad("attribute range overflow"))?;
        next_path = next_path
            .checked_add(head.path_len)
            .ok_or_else(|| bad("path range overflow"))?;
        if next_obj > object_count || next_attr > attr_count {
            return Err(bad("model range exceeds the object or attribute table"));
        }
        heads.push(head);
    }
    if next_obj != object_count || next_attr != attr_count {
        return Err(bad("object or attribute table has unused entries"));
    }
    if align4(next_path)? != strings_len {
        return Err(bad("string area length differs from the paths"));
    }
    let strings = rd.at(strings_offset, strings_len)?;
    if strings
        .get(next_path..)
        .is_some_and(|pad| pad.iter().any(|&b| b != 0))
    {
        return Err(bad("string padding is not zero"));
    }

    // pass 2: objects and data, tiling the data area exactly
    let mut set = CtrlModelSet::default();
    let mut cursor = 0usize; // relative to data_offset
    let data_len = strings_offset - data_offset;
    for h in &heads {
        let raw_path = strings
            .get(h.path_offset..h.path_offset + h.path_len)
            .ok_or_else(|| bad("path out of range"))?;
        let path = core::str::from_utf8(raw_path).map_err(|_| bad("path is not UTF-8"))?;
        let mut objects: Vec<RigidObject> = Vec::with_capacity(h.object_count.min(4096));
        let mut any_frozen = false;
        for k in 0..h.object_count {
            let o = objects_off + (h.first_object + k) * OBJECT_DESC_LEN;
            let parent = rd.i32(o)?;
            let kind = rd.u32(o + 4)?;
            let frozen = match kind {
                OBJECT_KIND_NORMAL => false,
                OBJECT_KIND_FROZEN => true,
                _ => return Err(bad("unknown object kind")),
            };
            if parent < -1 || (parent >= 0 && us(parent as u32) >= k) {
                return Err(bad("a parent does not precede its child"));
            }
            let mut tm = [0.0_f32; 16];
            for (j, slot) in tm.iter_mut().enumerate() {
                *slot = rd.f32(o + 8 + j * 4)?;
            }
            let vertex_count = us(rd.u32(o + 72)?);
            let triangle_count = us(rd.u32(o + 76)?);
            let vertex_offset = us(rd.u32(o + 80)?);
            let index_offset = us(rd.u32(o + 84)?);
            let frame_count = us(rd.u32(o + 88)?);
            let frames_offset = us(rd.u32(o + 92)?);
            if vertex_count > 65_536 {
                return Err(bad("more than 65536 vertices in one object"));
            }
            if frame_count != 0 && frame_count != us(h.max_frame) {
                return Err(bad("frame_count is neither 0 nor max_frame"));
            }
            if frozen && frame_count != 0 {
                return Err(bad("a frozen object carries node keys"));
            }
            any_frozen |= frozen;
            // the three blocks tile the data area in order
            let pos_bytes = vertex_count
                .checked_mul(12)
                .ok_or_else(|| bad("vertex block overflow"))?;
            let idx_bytes = align4(
                triangle_count
                    .checked_mul(6)
                    .ok_or_else(|| bad("index block overflow"))?,
            )?;
            let key_bytes = frame_count
                .checked_mul(KEY_LEN)
                .ok_or_else(|| bad("key block overflow"))?;
            if vertex_offset != cursor {
                return Err(bad("vertex block is not at the running data offset"));
            }
            if index_offset != cursor + pos_bytes {
                return Err(bad("index block does not follow the vertices"));
            }
            if frames_offset != index_offset + idx_bytes {
                return Err(bad("key block does not follow the indices"));
            }
            cursor = frames_offset
                .checked_add(key_bytes)
                .ok_or_else(|| bad("data offset overflow"))?;
            if cursor > data_len {
                return Err(bad("object data exceeds the data area"));
            }
            let pbase = data_offset + vertex_offset;
            let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vertex_count);
            for v in 0..vertex_count {
                let p = rd.vec3(pbase + v * 12)?;
                positions.push([p.x, p.y, p.z]);
            }
            let ibase = data_offset + index_offset;
            let mut tris: Vec<[u16; 3]> = Vec::with_capacity(triangle_count);
            for t in 0..triangle_count {
                let at = ibase + t * 6;
                let tri = [rd.u16(at)?, rd.u16(at + 2)?, rd.u16(at + 4)?];
                if tri.iter().any(|&i| us(u32::from(i)) >= vertex_count) {
                    return Err(bad("triangle index outside the vertex list"));
                }
                tris.push(tri);
            }
            if rd
                .at(ibase + triangle_count * 6, idx_bytes - triangle_count * 6)?
                .iter()
                .any(|&b| b != 0)
            {
                return Err(bad("index padding is not zero"));
            }
            let kbase = data_offset + frames_offset;
            let mut frames = Vec::with_capacity(frame_count);
            for f in 0..frame_count {
                let at = kbase + f * KEY_LEN;
                let q = Quat::new(
                    rd.f32(at)?,
                    rd.f32(at + 4)?,
                    rd.f32(at + 8)?,
                    rd.f32(at + 12)?,
                );
                frames.push((q, rd.vec3(at + 16)?));
            }
            objects.push(RigidObject {
                parent,
                frozen,
                local_tm: flyff_common::math::D3dMatrix(tm),
                mesh: CollisionMesh {
                    positions,
                    tris,
                    ..CollisionMesh::default()
                },
                frames,
            });
        }
        if any_frozen && h.flags & MODEL_BONE_PARENTED_FROZEN == 0 {
            return Err(bad("frozen objects without the model flag"));
        }
        // attributes: flagged entries, ascending unique slot < max_frame
        let mut attrs: Vec<MotionAttr> = Vec::with_capacity(h.attr_count.min(65_536));
        for a in 0..h.attr_count {
            let o = attrs_off + (h.first_attr + a) * ATTR_LEN;
            let attr = MotionAttr {
                index: rd.u32(o)?,
                flags: rd.u32(o + 4)?,
                sound: rd.i32(o + 8)?,
                frame: rd.f32(o + 12)?,
            };
            if attr.flags == 0 || attr.index >= h.max_frame {
                return Err(bad("attribute without flags or beyond max_frame"));
            }
            if attrs
                .last()
                .is_some_and(|p: &MotionAttr| p.index >= attr.index)
            {
                return Err(bad("attributes are not sorted by unique index"));
            }
            attrs.push(attr);
        }
        set.models.insert(
            h.ctrl_index,
            CtrlModel {
                ctrl_index: h.ctrl_index,
                path: path.to_owned(),
                objects,
                max_frame: h.max_frame,
                per_slerp_raw: h.per_slerp_raw,
                attrs,
                flags: h.flags,
                aabb: h.aabb,
            },
        );
    }
    if cursor != data_len {
        return Err(bad("the data area has unused bytes"));
    }
    Ok(set)
}
