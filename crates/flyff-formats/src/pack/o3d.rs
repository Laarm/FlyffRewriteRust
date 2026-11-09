//! The `.o3d` mesh pack (`pack::o3d::build`, Plan/30 §13.2): browser-oriented per-material-block geometry.
//!
//! Per GMOBJECT of every selected LOD group the pack holds `position`, `normal` (f32 x 3) and `uv` (f32 x 2)
//! sections; for skinned objects `joints` (u16 x 2, **global `.chr` bone indices**, palette-resolved) and
//! `weights` (f32 x 2); the `index` section (u16, or u32 when vertex duplication pushes the count above
//! 65,536); and, for node-animated rigid objects, `anim` (f32 x 7 per frame: `qx qy qz qw px py pz`).
//! Submeshes are the file's material blocks in order. The embedded skeleton/motion (35 files) and the node
//! clip are `FSK1`/`FMO1` sections ([`crate::pack::skeleton`], [`crate::pack::motion`]), always
//! left-handed and never mapped.
//!
//! Algorithm per GMOBJECT (Plan/30 §13.2):
//! 1. positions, normals, UVs of the vertex buffer (rigid: object-local; skinned: bind-pose model space);
//! 2. skinned: joints are resolved for every (block, vertex) pair referenced by the block's index range with
//!    `palette = block.use_bone if block.n_use_bone > 0, else object.use_bone if non-empty, else identity`
//!    and `bone = palette[idx / 3]`; a vertex referenced by two blocks that resolve to different bones is
//!    duplicated for the later block (`PaletteConflict`, corpus: 0); `idx % 3 != 0` is
//!    `PaletteIndexNotMultipleOf3`, a slot outside the palette `PaletteSlotOutOfRange` (joint 0);
//!    `w2 == 0` sets joint B = joint A; `physique[iib[v]] != joint A` is `PhysiqueMismatch`;
//! 3. submeshes: `start = startIndex`, `count = 3 * primCount`; blocks reaching outside the index buffer are
//!    dropped (`BlockRangeOutOfBounds`); `texture` is the lower-cased material name or `None` (a
//!    `textureID` outside the material list is `TextureIdOutOfRange`);
//! 4. [`Space::Three`]: positions, normals, `local_tm`, node keys, boxes, events and force points are
//!    mapped with [`crate::space`] and the winding of every triangle is reversed; `mirrored =
//!    det3(local_tm) < 0` is computed in the original left-handed space before mapping.
//!
//! The renderer composes rigid objects as `update[i] * World` (ref/18 §4.8, §7.3). Per-instance texture
//! overrides (`-etNN`, `_Flash`, skin sets) are name rules of the renderer (ref/18 §8.5).

use std::collections::BTreeMap;

use super::blob::{BlobBuilder, BlobView, Packed};
use super::{LodSelect, Space, motion, skeleton};
use crate::FormatKind;
use crate::error::{ErrorKind, FormatError, WarningCode};
use crate::limits::MAX_RENDER_VERTICES;
use crate::motion::MotionAttr;
use crate::o3d::{
    AttrBlock, GmBody, GmObject, GmType, MaterialBlock, O3dFile, SkinVertex, Vertices, name_matches,
};
use crate::raw::Vec3;
use crate::space::{
    det3, flip_winding_u16, flip_winding_u32, mirror_aabb, mirror_mat, mirror_pos, mirror_quat,
};

/// Options of [`build`] (Plan/30 §13.2).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(default))]
pub struct O3dPackOptions {
    /// Output space (default [`Space::Three`]).
    pub space: Space,
    /// LOD groups to emit (default [`LodSelect::All`]).
    pub lods: LodSelect,
    /// Also emit the collision body as [`MeshSections`] (default `false`).
    pub collision: bool,
    /// Fail with `Inconsistent { what: "o3d header name mismatch" }` when `requested_name` is set and the
    /// header name differs (default `true`: the original refuses to load such models, ledger KQ-42).
    pub reject_name_mismatch: bool,
    /// The file name the model was requested under (directories are ignored).
    pub requested_name: Option<String>,
}

impl Default for O3dPackOptions {
    fn default() -> O3dPackOptions {
        O3dPackOptions {
            space: Space::Three,
            lods: LodSelect::All,
            collision: false,
            reject_name_mismatch: true,
            requested_name: None,
        }
    }
}

/// Kind of an object in the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum ObjectKind {
    /// Rigid (`GMT_NORMAL` and any unknown kind).
    Rigid,
    /// Skinned (`GMT_SKIN`).
    Skin,
}

/// What an object is parented to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(rename_all = "lowercase"))]
pub enum ParentKind {
    /// A root object (`index == -1`).
    None,
    /// Another object of the same LOD group (it precedes this one).
    Object,
    /// A bone of the embedded skeleton, or of the external `.chr` skeleton.
    Bone,
}

/// Parent reference of an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ParentRef {
    /// Kind of parent.
    pub kind: ParentKind,
    /// Object or bone index (`-1` for none).
    pub index: i32,
}

/// One material block (draw call).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Submesh {
    /// First index in the object's index section.
    pub start: u32,
    /// Index count (`3 * primCount`).
    pub count: u32,
    /// The block's `textureID`.
    pub material: i32,
    /// Lower-cased texture file name, `None` when untextured.
    pub texture: Option<String>,
    /// `XE_*` effect bits.
    pub effect: u32,
    /// Opacity amount (meaningful with `XE_OPACITY`).
    pub amount: i32,
}

/// A non-empty frame attribute of the built-in animation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AttrMeta {
    /// Array index (`(int)frame`).
    pub index: u32,
    /// Exact frame.
    pub frame: f32,
    /// `MA_HIT`.
    pub hit: bool,
    /// `MA_SOUND`.
    pub sound: bool,
    /// `MA_QUAKE`.
    pub quake: bool,
    /// Sound id.
    pub sound_id: i32,
}

/// One object of a LOD group.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ObjectMeta {
    /// Rigid or skinned.
    pub kind: ObjectKind,
    /// Light object (bit 31): drawn additively at night only.
    pub light: bool,
    /// Parent reference.
    pub parent: ParentRef,
    /// Local transform (mapped when the pack is in three.js space).
    pub local_tm: [f32; 16],
    /// `det3(local_tm) < 0` in the original space (29 corpus objects).
    pub mirrored: bool,
    /// Object-local bounding box `[minx, miny, minz, maxx, maxy, maxz]`.
    pub bbox: [f32; 6],
    /// Vertex count (including duplicates created for palette conflicts).
    pub vertex_count: u32,
    /// `f32 x 3` per vertex.
    pub position: BlobView,
    /// `f32 x 3` per vertex.
    pub normal: BlobView,
    /// `f32 x 2` per vertex.
    pub uv: BlobView,
    /// `u16 x 2` global bone indices (skinned objects only).
    pub joints: Option<BlobView>,
    /// `f32 x 2` weights (skinned objects only).
    pub weights: Option<BlobView>,
    /// Triangle list, `u16` or `u32`.
    pub index: BlobView,
    /// The material blocks.
    pub submeshes: Vec<Submesh>,
    /// Node animation keys, `f32 x 7` per frame (rigid, animated files).
    pub anim: Option<BlobView>,
}

/// One LOD group.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LodMeta {
    /// Objects in file order (parents precede children).
    pub objects: Vec<ObjectMeta>,
}

/// Embedded skeleton sections (`FSK1` and, with a motion, `FMO1`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EmbeddedMeta {
    /// Bone count.
    pub bone_count: u32,
    /// The `FSK1` section (`U8`).
    pub skeleton: BlobView,
    /// The `FMO1` section (`U8`) of the embedded motion.
    pub motion: Option<BlobView>,
}

/// Position and index sections of the collision body.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MeshSections {
    /// `f32 x 3` positions (model space).
    pub position: BlobView,
    /// Triangle list `u16`.
    pub index: BlobView,
}

/// The structured description of a mesh pack (Plan/30 §13.2).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct O3dMeta {
    /// File version.
    pub version: i32,
    /// The space the pack was emitted in.
    pub space: Space,
    /// The header name (non-ASCII bytes shown as U+FFFD).
    pub header_name: String,
    /// `true` when the header name matches the requested name (always `true` without one).
    pub name_matches: bool,
    /// Model bounding box `[minx, miny, minz, maxx, maxy, maxz]`.
    pub bbox: [f32; 6],
    /// Built-in animation advance per tick.
    pub per_slerp: f32,
    /// Frames of the built-in animation (0 = static).
    pub max_frame: i32,
    /// Sword-trail points: none when `force1.x == 0`, else 2 (v20/21) or 4 (v22).
    pub force: Vec<Vec3>,
    /// Event points.
    pub events: Vec<Vec3>,
    /// Non-empty frame attributes of the built-in animation.
    pub attrs: Vec<AttrMeta>,
    /// The LOD groups.
    pub lods: Vec<LodMeta>,
    /// Embedded skeleton/motion (35 files).
    pub embedded: Option<EmbeddedMeta>,
    /// Node clip: an `FMO1` with flag bit 2 for node-animated files.
    pub node_clip: Option<BlobView>,
    /// Collision body when requested and present.
    pub collision: Option<MeshSections>,
    /// Pack-time warnings, one line each: `"<WarningCode>: LOD<g> object <o>: <detail>"`.
    pub warnings: Vec<String>,
}

fn err(what: &'static str) -> FormatError {
    FormatError::new(FormatKind::O3d, 0, ErrorKind::Inconsistent { what })
}

fn warn(warnings: &mut Vec<String>, code: WarningCode, where_: &str, detail: String) {
    warnings.push(format!("{code:?}: {where_}: {detail}"));
}

/// Builds the mesh pack of `file` (Plan/30 §13.2); see the module docs for the algorithm.
///
/// Errors (`Inconsistent`): header name mismatch (when requested and rejected), skipped geometry, indices
/// beyond the vertex buffer, an object parent that does not precede the object or a bone parent outside the
/// embedded skeleton, and failures of the embedded skeleton/motion/node-clip packs.
pub fn build(file: &O3dFile, opts: &O3dPackOptions) -> Result<Packed<O3dMeta>, FormatError> {
    let matches = opts
        .requested_name
        .as_deref()
        .is_none_or(|n| name_matches(file, n));
    if opts.reject_name_mismatch && !matches {
        return Err(err("o3d header name mismatch"));
    }
    let three = opts.space == Space::Three;
    let map_pos = |p: Vec3| if three { mirror_pos(p) } else { p };
    let mut bb = BlobBuilder::for_format(FormatKind::O3d);
    let mut warnings = Vec::new();
    let embedded_bones = file.bones.as_ref().map(|b| b.base.len());

    let take = match opts.lods {
        LodSelect::All => file.groups.len(),
        LodSelect::Lod0 => file.groups.len().min(1),
    };
    let mut lods = Vec::with_capacity(take);
    for (gi, group) in file.groups.iter().take(take).enumerate() {
        let mut objects = Vec::with_capacity(group.objects.len());
        for (oi, obj) in group.objects.iter().enumerate() {
            let ctx = ObjCtx {
                three,
                group_len: group.objects.len(),
                index_in_group: oi,
                embedded_bones,
                label: format!("LOD{gi} object {oi}"),
            };
            objects.push(build_object(obj, &ctx, &mut bb, &mut warnings)?);
        }
        lods.push(LodMeta { objects });
    }

    let embedded = match skeleton::build_from_o3d(file)? {
        Some(skel) => {
            let skeleton = bb.push_bytes(&skel)?;
            let motion = motion::build_from_o3d(file)?
                .map(|m| bb.push_bytes(&m))
                .transpose()?;
            Some(EmbeddedMeta {
                bone_count: embedded_bones.unwrap_or(0) as u32,
                skeleton,
                motion,
            })
        }
        None => None,
    };
    let node_clip = motion::build_nodes_from_o3d(file)?
        .map(|m| bb.push_bytes(&m))
        .transpose()?;

    let collision = match (&file.collision, opts.collision) {
        (Some(body), true) => Some(build_collision(body, three, &mut bb)?),
        _ => None,
    };

    let (bmin, bmax) = if three {
        mirror_aabb(file.bb_min, file.bb_max)
    } else {
        (file.bb_min, file.bb_max)
    };
    let force = if file.force[0][0] == 0.0 {
        Vec::new()
    } else {
        let n = if file.version >= crate::o3d::consts::VER_FORCE34 {
            4
        } else {
            2
        };
        file.force.iter().take(n).map(|p| map_pos(*p)).collect()
    };
    let attrs = match &file.attrs {
        AttrBlock::Attrs(list) => list
            .iter()
            .enumerate()
            .filter(|(_, a)| a.flags != 0)
            .map(|(i, a)| attr_meta(i, a))
            .collect(),
        _ => Vec::new(),
    };
    let meta = O3dMeta {
        version: file.version,
        space: opts.space,
        header_name: file.header_name.to_string_lossy_ascii(),
        name_matches: matches,
        bbox: [bmin[0], bmin[1], bmin[2], bmax[0], bmax[1], bmax[2]],
        per_slerp: file.per_slerp,
        max_frame: file.max_frame,
        force,
        events: file.events.iter().map(|p| map_pos(*p)).collect(),
        attrs,
        lods,
        embedded,
        node_clip,
        collision,
        warnings,
    };
    Ok(Packed {
        meta,
        blob: bb.finish(),
    })
}

pub(crate) fn attr_meta(index: usize, a: &MotionAttr) -> AttrMeta {
    AttrMeta {
        index: index as u32,
        frame: a.frame,
        hit: a.is_hit(),
        sound: a.is_sound(),
        quake: a.is_quake(),
        sound_id: a.sound_id,
    }
}

struct ObjCtx {
    three: bool,
    group_len: usize,
    index_in_group: usize,
    embedded_bones: Option<usize>,
    label: String,
}

/// Per-object counters of the tolerated irregularities (one summary warning each).
#[derive(Default)]
struct Stats {
    not_multiple_of_3: usize,
    slot_out_of_range: usize,
    conflicts: usize,
    physique_mismatch: usize,
    bad_blocks: usize,
    bad_texture_ids: usize,
}

fn build_object(
    obj: &GmObject,
    ctx: &ObjCtx,
    bb: &mut BlobBuilder,
    warnings: &mut Vec<String>,
) -> Result<ObjectMeta, FormatError> {
    let body = &obj.body;
    let kind = match obj.kind() {
        GmType::Skin => ObjectKind::Skin,
        GmType::Normal | GmType::Other(_) => ObjectKind::Rigid,
    };
    let n = body.vertices.len();
    let mut positions: Vec<f32> = Vec::with_capacity(n * 3);
    let mut normals: Vec<f32> = Vec::with_capacity(n * 3);
    let mut uvs: Vec<f32> = Vec::with_capacity(n * 2);
    let mut skin: Option<&[SkinVertex]> = None;
    match (&body.vertices, kind) {
        (Vertices::Skin(v), ObjectKind::Skin) => {
            skin = Some(v);
            for sv in v {
                positions.extend_from_slice(&sv.pos);
                normals.extend_from_slice(&sv.normal);
                uvs.extend_from_slice(&sv.uv);
            }
        }
        (Vertices::Normal(v), ObjectKind::Rigid) => {
            for nv in v {
                positions.extend_from_slice(&nv.pos);
                normals.extend_from_slice(&nv.normal);
                uvs.extend_from_slice(&nv.uv);
            }
        }
        (Vertices::Skipped, _) => {
            return Err(err(
                "o3d geometry was skipped (parse with GeometryMode::Full)",
            ));
        }
        _ => return Err(err("o3d object kind does not match its vertex layout")),
    }
    if n > MAX_RENDER_VERTICES as usize {
        return Err(err("o3d object has more than 65,536 vertices"));
    }
    if body.indices.iter().any(|&i| usize::from(i) >= n) {
        return Err(err("o3d index beyond the vertex buffer"));
    }

    let mut stats = Stats::default();
    let mut indices: Vec<u32> = body.indices.iter().map(|&i| u32::from(i)).collect();

    // Valid blocks (in file order) and their submeshes.
    let materials = body.materials.as_ref();
    let mut valid_blocks = Vec::with_capacity(body.blocks.len());
    let mut submeshes = Vec::with_capacity(body.blocks.len());
    for blk in &body.blocks {
        let start = i64::from(blk.start_index);
        let count = i64::from(blk.prim_count) * 3;
        if start < 0 || count < 0 || start + count > indices.len() as i64 {
            stats.bad_blocks += 1;
            continue;
        }
        let texture = match materials {
            Some(list) => match usize::try_from(blk.texture_id)
                .ok()
                .and_then(|i| list.materials.get(i))
            {
                Some(m) if !m.texture.is_empty() => {
                    Some(m.texture.to_ascii_lowercase().to_string_lossy_ascii())
                }
                Some(_) => None,
                None => {
                    stats.bad_texture_ids += 1;
                    None
                }
            },
            None => None,
        };
        submeshes.push(Submesh {
            start: start as u32,
            count: count as u32,
            material: blk.texture_id,
            texture,
            effect: blk.effect,
            amount: blk.amount,
        });
        valid_blocks.push((blk, start as usize, count as usize));
    }

    // Skinned: palette resolution with conflict duplication.
    let mut joints: Vec<u16> = Vec::new();
    let mut weights: Vec<f32> = Vec::new();
    if let Some(sv) = skin {
        let r = resolve_skin(obj, body, sv, &valid_blocks, &mut indices, &mut stats);
        joints = r.joints;
        weights = r.weights;
        for &src in &r.duplicates {
            if let Some(v) = sv.get(src) {
                positions.extend_from_slice(&v.pos);
                normals.extend_from_slice(&v.normal);
                uvs.extend_from_slice(&v.uv);
            }
        }
    }
    let vertex_count = positions.len() / 3;

    // Mapping to three.js space (positions and normals: z negated).
    if ctx.three {
        for v in positions
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .chain(normals.as_chunks_mut::<3>().0.iter_mut())
        {
            v[2] = -v[2]; // BOUNDS: a [f32; 3] chunk
        }
    }
    let wide = vertex_count > MAX_RENDER_VERTICES as usize;
    let position = bb.push_f32(3, &positions)?;
    let normal = bb.push_f32(3, &normals)?;
    let uv = bb.push_f32(2, &uvs)?;
    let (joints_view, weights_view) = if skin.is_some() {
        (
            Some(bb.push_u16(2, &joints)?),
            Some(bb.push_f32(2, &weights)?),
        )
    } else {
        (None, None)
    };
    let index = if wide {
        if ctx.three {
            flip_winding_u32(&mut indices);
        }
        bb.push_u32(1, &indices)?
    } else {
        let mut narrow: Vec<u16> = indices.iter().map(|&i| i as u16).collect();
        if ctx.three {
            flip_winding_u16(&mut narrow);
        }
        bb.push_u16(1, &narrow)?
    };

    // Node animation.
    let anim = match &obj.node_anim {
        Some(na) if na.has_frames != 0 && !na.keys.is_empty() => {
            let mut flat = Vec::with_capacity(na.keys.len() * 7);
            for k in &na.keys {
                let (rot, pos) = if ctx.three {
                    (mirror_quat(k.rot), mirror_pos(k.pos))
                } else {
                    (k.rot, k.pos)
                };
                flat.extend_from_slice(&rot);
                flat.extend_from_slice(&pos);
            }
            Some(bb.push_f32(7, &flat)?)
        }
        _ => None,
    };

    // Parent reference.
    let parent = if obj.parent_idx == -1 {
        ParentRef {
            kind: ParentKind::None,
            index: -1,
        }
    } else if obj.parent_type == Some(2) {
        let ok = usize::try_from(obj.parent_idx)
            .ok()
            .is_some_and(|i| ctx.embedded_bones.is_none_or(|n| i < n));
        if !ok {
            return Err(err("o3d object parent bone is out of range"));
        }
        ParentRef {
            kind: ParentKind::Bone,
            index: obj.parent_idx,
        }
    } else {
        let ok = usize::try_from(obj.parent_idx)
            .ok()
            .is_some_and(|i| i < ctx.index_in_group && i < ctx.group_len);
        if !ok {
            return Err(err("o3d object parent does not precede the object"));
        }
        ParentRef {
            kind: ParentKind::Object,
            index: obj.parent_idx,
        }
    };

    let mirrored = det3(&obj.local_tm) < 0.0;
    let local_tm = if ctx.three {
        mirror_mat(&obj.local_tm)
    } else {
        obj.local_tm
    };
    let (bmin, bmax) = if ctx.three {
        mirror_aabb(body.bb_min, body.bb_max)
    } else {
        (body.bb_min, body.bb_max)
    };

    summarize(&stats, &ctx.label, warnings);
    Ok(ObjectMeta {
        kind,
        light: obj.is_light(),
        parent,
        local_tm: local_tm.0,
        mirrored,
        bbox: [bmin[0], bmin[1], bmin[2], bmax[0], bmax[1], bmax[2]],
        vertex_count: vertex_count as u32,
        position,
        normal,
        uv,
        joints: joints_view,
        weights: weights_view,
        index,
        submeshes,
        anim,
    })
}

fn summarize(stats: &Stats, label: &str, warnings: &mut Vec<String>) {
    let table = [
        (
            stats.conflicts,
            WarningCode::PaletteConflict,
            "vertices duplicated for a second material block palette",
        ),
        (
            stats.not_multiple_of_3,
            WarningCode::PaletteIndexNotMultipleOf3,
            "bone index references not a multiple of 3 (idx / 3 used)",
        ),
        (
            stats.slot_out_of_range,
            WarningCode::PaletteSlotOutOfRange,
            "bone index references outside the palette (joint 0 used)",
        ),
        (
            stats.physique_mismatch,
            WarningCode::PhysiqueMismatch,
            "vertices whose primary bone differs from the physique table",
        ),
        (
            stats.bad_blocks,
            WarningCode::BlockRangeOutOfBounds,
            "material blocks outside the index buffer dropped",
        ),
        (
            stats.bad_texture_ids,
            WarningCode::TextureIdOutOfRange,
            "material blocks with a texture id outside the material list",
        ),
    ];
    for (count, code, what) in table {
        if count > 0 {
            warn(warnings, code, label, format!("{count} {what}"));
        }
    }
}

/// Palette used to resolve a vertex's bone slots.
#[derive(Clone, Copy)]
enum Palette<'a> {
    Slots(&'a [i32]),
    Identity,
}

fn resolve_slot(idx: u16, palette: Palette<'_>, stats: &mut Stats) -> u16 {
    if !idx.is_multiple_of(3) {
        stats.not_multiple_of_3 += 1;
    }
    let slot = usize::from(idx / 3);
    match palette {
        Palette::Identity => slot as u16,
        Palette::Slots(p) => match p.get(slot).and_then(|&b| u16::try_from(b).ok()) {
            Some(b) => b,
            None => {
                stats.slot_out_of_range += 1;
                0
            }
        },
    }
}

/// Resolves `(joint A, joint B)` of vertex `sv` through `palette`.
fn resolve_vertex(sv: &SkinVertex, palette: Palette<'_>, stats: &mut Stats) -> (u16, u16) {
    let a = resolve_slot(sv.idx_a, palette, stats);
    // `w2 == 0`: idxB may point anywhere and is ignored.
    let b = if sv.w2 == 0.0 {
        a
    } else {
        resolve_slot(sv.idx_b, palette, stats)
    };
    (a, b)
}

/// Result of the joint resolution of one skinned object.
struct SkinResult {
    /// `u16 x 2` per vertex: the original vertices, then the duplicates.
    joints: Vec<u16>,
    /// `f32 x 2` per vertex, same order.
    weights: Vec<f32>,
    /// Source vertex of each duplicate (duplicate `d` has index `n + d`).
    duplicates: Vec<usize>,
}

/// Joint resolution with conflict duplication: rewrites the index ranges of the conflicting blocks in
/// `indices` and reports the duplicated vertices (the caller appends their attribute data).
fn resolve_skin(
    obj: &GmObject,
    body: &GmBody,
    verts: &[SkinVertex],
    blocks: &[(&MaterialBlock, usize, usize)],
    indices: &mut [u32],
    stats: &mut Stats,
) -> SkinResult {
    let n = verts.len();
    let original = indices.to_vec();
    let object_palette = if obj.use_bone.is_empty() {
        Palette::Identity
    } else {
        Palette::Slots(&obj.use_bone)
    };
    let mut assigned: Vec<Option<(u16, u16)>> = vec![None; n];
    // (source vertex, joint A, joint B) -> index of the duplicate.
    let mut dups: BTreeMap<(usize, u16, u16), usize> = BTreeMap::new();
    let mut dup_list: Vec<(usize, u16, u16)> = Vec::new();

    for (blk, start, count) in blocks {
        let palette = if blk.n_use_bone > 0 {
            let used = usize::try_from(blk.n_use_bone)
                .unwrap_or(0)
                .min(blk.use_bone.len());
            Palette::Slots(blk.use_bone.get(..used).unwrap_or(&[]))
        } else {
            object_palette
        };
        for k in *start..*start + *count {
            let Some(&v) = original.get(k) else { continue };
            let v = v as usize;
            let (Some(sv), Some(slot)) = (verts.get(v), assigned.get_mut(v)) else {
                continue;
            };
            let (a, b) = resolve_vertex(sv, palette, stats);
            match *slot {
                None => *slot = Some((a, b)),
                Some(prev) if prev == (a, b) => {}
                Some(_) => {
                    let new_index = *dups.entry((v, a, b)).or_insert_with(|| {
                        dup_list.push((v, a, b));
                        stats.conflicts += 1;
                        n + dup_list.len() - 1
                    });
                    if let Some(dst) = indices.get_mut(k) {
                        *dst = new_index as u32;
                    }
                }
            }
        }
    }

    let mut joints: Vec<u16> = Vec::with_capacity((n + dup_list.len()) * 2);
    let mut weights: Vec<f32> = Vec::with_capacity((n + dup_list.len()) * 2);
    for (v, sv) in verts.iter().enumerate() {
        let (a, b) = match assigned.get(v).copied().flatten() {
            Some(pair) => pair,
            // Unreferenced vertices use the object palette (or identity).
            None => resolve_vertex(sv, object_palette, stats),
        };
        joints.extend_from_slice(&[a, b]);
        weights.extend_from_slice(&[sv.w1, sv.w2]);
        check_physique(body, v, a, stats);
    }
    for &(src, a, b) in &dup_list {
        if let Some(sv) = verts.get(src) {
            joints.extend_from_slice(&[a, b]);
            weights.extend_from_slice(&[sv.w1, sv.w2]);
        }
        check_physique(body, src, a, stats);
    }
    SkinResult {
        joints,
        weights,
        duplicates: dup_list.iter().map(|d| d.0).collect(),
    }
}

/// `physique[iib[v]]` must equal joint A (corpus: always).
fn check_physique(body: &GmBody, v: usize, joint_a: u16, stats: &mut Stats) {
    if body.physique.is_empty() {
        return;
    }
    let primary = body
        .iib
        .get(v)
        .and_then(|&u| body.physique.get(usize::from(u)));
    if let Some(&p) = primary
        && p != i32::from(joint_a)
    {
        stats.physique_mismatch += 1;
    }
}

fn build_collision(
    body: &GmBody,
    three: bool,
    bb: &mut BlobBuilder,
) -> Result<MeshSections, FormatError> {
    let Vertices::Normal(v) = &body.vertices else {
        return Err(err(
            "o3d collision geometry was skipped or has skin vertices",
        ));
    };
    if v.len() > MAX_RENDER_VERTICES as usize
        || body.indices.iter().any(|&i| usize::from(i) >= v.len())
    {
        return Err(err("o3d collision index beyond the vertex buffer"));
    }
    let mut positions = Vec::with_capacity(v.len() * 3);
    for nv in v {
        let p = if three { mirror_pos(nv.pos) } else { nv.pos };
        positions.extend_from_slice(&p);
    }
    let mut indices = body.indices.clone();
    if three {
        flip_winding_u16(&mut indices);
    }
    Ok(MeshSections {
        position: bb.push_f32(3, &positions)?,
        index: bb.push_u16(1, &indices)?,
    })
}
