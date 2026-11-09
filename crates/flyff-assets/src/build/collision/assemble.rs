//! `FLYC` content assembly (Plan/24 §10.7; WP-WEB-13): parsed lands and models in, [`LandChunk`] values
//! out. Pure and deterministic, no I/O: the bytes come only from `flyff_sim::encode_chunk` (W9).
//!
//! The assembly follows the original server's world loader (`CWorld::ReadWorld`,
//! src:_Common/WorldFile.cpp:823-955) as the plan transcribes it:
//!
//! * **Terrain** (§10.7.2): the server keeps one global height array without a 129th row/column, so the
//!   vertex `x = 128` (`z = 128`, the corner) of a land is the neighbour's column 0 (row 0, vertex (0, 0)):
//!   the neighbour's raw value when its `.lnd` exists, raw `0.0` when the neighbour is inside the world but
//!   has no file, and the land's own value when the neighbour is outside the world. A land without a file
//!   has raw `0.0` everywhere else, so it is uniform unless an existing +x/+z neighbour contributes its
//!   edge. Height = `strip_height_server(raw)`, attribute = classification of the **raw** value.
//! * **Instances** (§10.7.5): lands in server load order (`z` outer, `x` inner), records in file order,
//!   only `OT_OBJ` records whose model has a collision body; `world = S·R·T`, `inv_world` in `f64`, OBB.
//! * **Link cells** (§10.7.6): level and cell by the original's `CalcLinkLevel`/`GetObjPtr` (the single
//!   implementation is `flyff_sim::collision::link_placement`), the instance lives in the chunk that owns
//!   its cell, list order "first inserted, then the others in reverse insertion order".

use flyff_common::math::D3dMatrix;
use flyff_formats::lnd::{HeightAttr, strip_height_server};
use flyff_formats::objects::OT_OBJ;
use flyff_sim::collision::chunk::{
    Attrs, Heights, INSTANCE_FLAG_ALWAYS_OBB, INSTANCE_FLAG_TRANS, LandChunk, LinkCell, MAP_SIZE,
    Obb, PATCH_COUNT, StaticInstance, VERT_COUNT, VERTS_PER_SIDE, link_width,
};
use flyff_sim::collision::geom::transform_coord;
use flyff_sim::collision::linkmap::link_placement;
use flyff_sim::num::{Vec3, to_radian};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub use flyff_sim::collision::chunk::CollisionMesh;

/// `OLD_MPU`: x and z of every stored position are multiplied by this, whatever the world's MPU
/// (src:_Common/Obj.cpp:522-523, ref/19 §7.1; KEEP-QUIRK).
const OLD_MPU: f32 = 4.0;

/// The file name whose instances use the always-hit OBB test (src:_Common/ModelObject.cpp:390-405).
const ALWAYS_OBB_MODEL: &str = "obj_guildcombatground.o3d";

/// Most instances one link cell can list (`CellDesc.count` is a `u16`; the original walks at most 1 000).
const MAX_CELL_LIST: usize = u16::MAX as usize;

/// Cap of the failure lists kept in a [`CollisionReport`].
pub const MAX_REPORT_FAILURES: usize = 64;

/// `SOBJ.flags` of an instance (Plan/24 §10.7.5 rule 6): bit 0 `ALWAYS_OBB` iff the resolved model file
/// is `obj_guildcombatground.o3d`, bit 1 `TRANS` iff the `mdlObj` entry's `trans` field is set.
/// `model_file` is the lower-case file name (`ModelRegistry::model_file`); the comparison ignores ASCII
/// case anyway.
pub fn instance_flags(model_file: &str, trans: bool) -> u8 {
    let mut flags = 0;
    if model_file.eq_ignore_ascii_case(ALWAYS_OBB_MODEL) {
        flags |= INSTANCE_FLAG_ALWAYS_OBB;
    }
    if trans {
        flags |= INSTANCE_FLAG_TRANS;
    }
    flags
}

/// One object record of a land file, as read (Plan/24 §10.7, `ObjectInput`).
#[derive(Clone, Debug)]
pub struct ObjectInput {
    /// Position of the record in its land file's object list (`SOBJ.src_ordinal`).
    pub ordinal: u32,
    /// The record's object type (`OT_*`); only `OT_OBJ` (0) is kept.
    pub obj_type: u32,
    /// `mdlObj` model index (`m_dwIndex`).
    pub model_index: u32,
    /// Position as stored in the file: x and z are still divided by `OLD_MPU`.
    pub pos_file: [f32; 3],
    /// `angle_y`, `ang_x`, `ang_z` in degrees as read.
    pub angles: [f32; 3],
    /// Scale.
    pub scale: [f32; 3],
    /// The collision body of the model; `None` = the model has no collision body or is not a mesh model
    /// (the record is dropped).
    pub mesh: Option<Arc<CollisionMesh>>,
    /// `SOBJ.flags` ([`instance_flags`]).
    pub flags: u8,
}

/// One land of a world (Plan/24 §10.7, `LandInput`).
#[derive(Clone, Debug)]
pub struct LandInput {
    /// Land column.
    pub x: u16,
    /// Land row.
    pub z: u16,
    /// The `.lnd` file existed (`LAND_PRESENT`).
    pub present: bool,
    /// The 129 x 129 raw heights as read (`z * 129 + x`, attribute offsets included). `None` when the
    /// land has no file or when the caller did not load its terrain (it is then never read: only the
    /// requested lands and their +x/+z neighbours need terrain).
    pub heights_raw: Option<Box<[f32]>>,
    /// `WATERHEIGHT[256]` as `{height, texture}` pairs (zeros for a missing land).
    pub water: [u8; 2 * PATCH_COUNT],
    /// The object list in file order.
    pub objects: Vec<ObjectInput>,
}

impl LandInput {
    /// A land without a file.
    pub fn missing(x: u16, z: u16) -> LandInput {
        LandInput {
            x,
            z,
            present: false,
            heights_raw: None,
            water: [0; 2 * PATCH_COUNT],
            objects: Vec::new(),
        }
    }
}

/// Everything `assemble_world` needs for one world folder (Plan/24 §10.7, `CollisionInputs`).
#[derive(Clone, Debug)]
pub struct CollisionInputs {
    /// The world folder (lower case).
    pub folder: String,
    /// World units per heightmap cell (`.wld` `MPU`: 1, 2 or 4).
    pub mpu: u8,
    /// World width in lands.
    pub lands_w: u16,
    /// World height in lands.
    pub lands_h: u16,
    /// Every coordinate, `z` outer, `x` inner (the server load order): `lands[z * lands_w + x]`.
    pub lands: Vec<LandInput>,
}

impl CollisionInputs {
    /// An all-missing world.
    pub fn empty(folder: &str, mpu: u8, lands_w: u16, lands_h: u16) -> CollisionInputs {
        let mut lands = Vec::with_capacity(usize::from(lands_w) * usize::from(lands_h));
        for z in 0..lands_h {
            for x in 0..lands_w {
                lands.push(LandInput::missing(x, z));
            }
        }
        CollisionInputs {
            folder: folder.to_owned(),
            mpu,
            lands_w,
            lands_h,
            lands,
        }
    }

    /// The land at `(x, z)`.
    pub fn land(&self, x: u16, z: u16) -> Option<&LandInput> {
        if x >= self.lands_w || z >= self.lands_h {
            return None;
        }
        let i = usize::from(z) * usize::from(self.lands_w) + usize::from(x);
        self.lands.get(i).filter(|l| l.x == x && l.z == z)
    }
}

/// Counters of one assembly or build (Plan/24 WP-WEB-24 `CollisionReport`): instances kept and dropped
/// by reason, triangles, terrain encodings and the dropped random-angle candidates. Every counter is a
/// sum over chunks, so reports of parts merge with [`CollisionReport::merge`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionReport {
    /// Chunks assembled.
    pub chunks: u32,
    /// Lands whose `.lnd` existed and was read.
    pub lands_present: u32,
    /// Lands without a file (uniform zero unless a +x/+z neighbour contributes its edge).
    pub lands_missing: u32,
    /// Lands whose file could not be read: treated as missing (Plan/24 §7.6), see `failures`.
    pub lands_failed: u32,
    /// Object records read (all types).
    pub objects_read: u64,
    /// Dropped: the record is not an `OT_OBJ`.
    pub dropped_not_obj: u64,
    /// Dropped: no collision mesh (sum of the four reasons below when the world builder classified it).
    pub dropped_no_mesh: u64,
    /// Of `dropped_no_mesh`: the model index is not in `mdlObj.inc`.
    pub no_mesh_unknown_model: u64,
    /// Of `dropped_no_mesh`: a billboard, sfx or ASE model (the original's `LoadModel` yields no mesh).
    pub no_mesh_non_mesh_model: u64,
    /// Of `dropped_no_mesh`: the `.o3d` is missing or unreadable (also counted per model in
    /// `models_failed`).
    pub no_mesh_unreadable_model: u64,
    /// Of `dropped_no_mesh`: the model has no collision body (`IsHaveCollObj() == FALSE`).
    pub no_mesh_no_collision_body: u64,
    /// Dropped: a non-finite position, angle or scale.
    pub dropped_non_finite: u64,
    /// Dropped: a singular world matrix (a zero scale), Plan/24 §10.7.5 rule 5.
    pub dropped_singular: u64,
    /// Dropped: the link-map cell does not exist (the original cannot link the object, §10.7.6 rule 2).
    pub dropped_unlinked: u64,
    /// Objects of models with `index <= 10`: the server's `SetAngle(rand() % 360)` is not reproduced
    /// (§10.7.5 rule 2); they are kept with their file angle.
    pub random_angle_candidates: u64,
    /// Instances kept.
    pub instances: u64,
    /// Collision triangles counted per kept instance.
    pub triangles: u64,
    /// Instances stored in a chunk other than the land file they came from (neighbour link cells).
    pub cross_land_instances: u64,
    /// Link cells (non-empty), all levels.
    pub link_cells: u64,
    /// Link cells whose list was cut at 65 535 entries.
    pub link_cell_overflow: u64,
    /// `SMSH` mesh entries over all chunks (meshes are deduplicated per chunk).
    pub meshes: u64,
    /// Triangles stored in those meshes.
    pub mesh_triangles: u64,
    /// Distinct models whose collision body was read.
    pub models_loaded: u32,
    /// Models whose `.o3d` could not be used.
    pub models_failed: u32,
    /// Chunks whose heights are `UNIFORM`.
    pub height_uniform: u32,
    /// Chunks whose heights are `Q16`.
    pub height_q16: u32,
    /// Chunks whose heights are `F32`.
    pub height_f32: u32,
    /// Chunks whose attributes are `UNIFORM`.
    pub attr_uniform: u32,
    /// Chunks whose attributes are `DENSE_U8`.
    pub attr_dense: u32,
    /// Non-finite raw heights replaced by `0.0`.
    pub nonfinite_heights: u64,
    /// Present lands whose terrain was needed but not supplied (read as raw `0.0`).
    pub terrain_unavailable: u32,
    /// Encoded bytes over all chunks (set by `build_world_chunks`).
    pub bytes: u64,
    /// Human-readable failures (unreadable lands and models), at most [`MAX_REPORT_FAILURES`].
    pub failures: Vec<String>,
}

impl CollisionReport {
    /// Adds `other` into `self` (sums; failures are appended up to the cap).
    pub fn merge(&mut self, other: &CollisionReport) {
        self.chunks += other.chunks;
        self.lands_present += other.lands_present;
        self.lands_missing += other.lands_missing;
        self.lands_failed += other.lands_failed;
        self.objects_read += other.objects_read;
        self.dropped_not_obj += other.dropped_not_obj;
        self.dropped_no_mesh += other.dropped_no_mesh;
        self.no_mesh_unknown_model += other.no_mesh_unknown_model;
        self.no_mesh_non_mesh_model += other.no_mesh_non_mesh_model;
        self.no_mesh_unreadable_model += other.no_mesh_unreadable_model;
        self.no_mesh_no_collision_body += other.no_mesh_no_collision_body;
        self.dropped_non_finite += other.dropped_non_finite;
        self.dropped_singular += other.dropped_singular;
        self.dropped_unlinked += other.dropped_unlinked;
        self.random_angle_candidates += other.random_angle_candidates;
        self.instances += other.instances;
        self.triangles += other.triangles;
        self.cross_land_instances += other.cross_land_instances;
        self.link_cells += other.link_cells;
        self.link_cell_overflow += other.link_cell_overflow;
        self.meshes += other.meshes;
        self.mesh_triangles += other.mesh_triangles;
        self.models_loaded += other.models_loaded;
        self.models_failed += other.models_failed;
        self.height_uniform += other.height_uniform;
        self.height_q16 += other.height_q16;
        self.height_f32 += other.height_f32;
        self.attr_uniform += other.attr_uniform;
        self.attr_dense += other.attr_dense;
        self.nonfinite_heights += other.nonfinite_heights;
        self.terrain_unavailable += other.terrain_unavailable;
        self.bytes += other.bytes;
        for f in &other.failures {
            self.fail(f.clone());
        }
    }

    /// Records a failure line (kept up to the cap).
    pub fn fail(&mut self, line: String) {
        if self.failures.len() < MAX_REPORT_FAILURES {
            self.failures.push(line);
        }
    }

    /// Objects dropped for any reason.
    pub fn dropped_total(&self) -> u64 {
        self.dropped_not_obj
            + self.dropped_no_mesh
            + self.dropped_non_finite
            + self.dropped_singular
            + self.dropped_unlinked
    }
}

// ---------------------------------------------------------------------------------------------------
// Matrices (Plan/24 §10.7.5 rules 3-5)
// ---------------------------------------------------------------------------------------------------

/// `-0.0` becomes `+0.0`: the derived matrices and OBB values of a record depend on its numeric content
/// only, not on the sign of a zero that `sin(-0.0)` or a product happened to produce.
fn canon(v: f32) -> f32 {
    if v == 0.0 { 0.0 } else { v }
}

fn canon3(v: Vec3) -> Vec3 {
    Vec3::new(canon(v.x), canon(v.y), canon(v.z))
}

/// `S · R · T` with `R = RotationYawPitchRoll(rad(-angle_y), rad(-ang_x), rad(+ang_z))` (the V15
/// `__FIX_ROTATE` sign, KEEP-QUIRK), `rad(a) = a * (pi_f32 / 180)`, and the three unscaled rotation rows.
/// `angles` are `[angle_y, ang_x, ang_z]` in degrees. Zeros are `+0.0` (see `canon`).
pub fn instance_matrices(pos: Vec3, angles: [f32; 3], scale: Vec3) -> (D3dMatrix, [Vec3; 3]) {
    let yaw = to_radian(-angles[0]);
    let pitch = to_radian(-angles[1]);
    let roll = to_radian(angles[2]);
    let (sy, cy) = (libm::sinf(yaw), libm::cosf(yaw));
    let (sp, cp) = (libm::sinf(pitch), libm::cosf(pitch));
    let (sr, cr) = (libm::sinf(roll), libm::cosf(roll));
    let r1 = Vec3::new(cr * cy + sr * sp * sy, sr * cp, -cr * sy + sr * sp * cy);
    let r2 = Vec3::new(-sr * cy + cr * sp * sy, cr * cp, sr * sy + cr * sp * cy);
    let r3 = Vec3::new(cp * sy, -sp, cp * cy);
    let rows = [
        canon3(r1 * scale.x),
        canon3(r2 * scale.y),
        canon3(r3 * scale.z),
    ];
    let world = D3dMatrix([
        rows[0].x, rows[0].y, rows[0].z, 0.0, //
        rows[1].x, rows[1].y, rows[1].z, 0.0, //
        rows[2].x, rows[2].y, rows[2].z, 0.0, //
        pos.x, pos.y, pos.z, 1.0,
    ]);
    (world, [canon3(r1), canon3(r2), canon3(r3)])
}

/// The inverse of `m` computed in `f64` from the `f32` entries and rounded to `f32` (§10.7.5); `None`
/// for a singular or non-finite matrix. Zeros are `+0.0`.
pub fn invert_f64(m: &D3dMatrix) -> Option<D3dMatrix> {
    let a: [f64; 16] = std::array::from_fn(|i| f64::from(m.0.get(i).copied().unwrap_or(0.0)));
    let g = |r: usize, c: usize| a.get(r * 4 + c).copied().unwrap_or(0.0);
    let others = |i: usize| -> [usize; 3] {
        let mut it = (0..4usize).filter(|j| *j != i);
        [
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
            it.next().unwrap_or(0),
        ]
    };
    let det3 = |r: [usize; 3], c: [usize; 3]| -> f64 {
        g(r[0], c[0]) * (g(r[1], c[1]) * g(r[2], c[2]) - g(r[1], c[2]) * g(r[2], c[1]))
            - g(r[0], c[1]) * (g(r[1], c[0]) * g(r[2], c[2]) - g(r[1], c[2]) * g(r[2], c[0]))
            + g(r[0], c[2]) * (g(r[1], c[0]) * g(r[2], c[1]) - g(r[1], c[1]) * g(r[2], c[0]))
    };
    let mut det = 0.0;
    for c in 0..4 {
        let sign = if c % 2 == 0 { 1.0 } else { -1.0 };
        det += g(0, c) * (det3(others(0), others(c)) * sign);
    }
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let mut out = [0.0_f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            let sign = if (r + c) % 2 == 0 { 1.0 } else { -1.0 };
            // inverse[c][r] = cofactor(r, c) / det
            let v = sign * det3(others(r), others(c)) / det;
            if let Some(slot) = out.get_mut(c * 4 + r) {
                *slot = canon(v as f32); // BOUNDS: the f64 -> f32 rounding is the specified one
            }
        }
    }
    if out.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(D3dMatrix(out))
}

/// The in-cell iteration order of the original's intrusive list (§10.7.6 rule 3): `InsertObjLink` puts a
/// new object after the head, so the order is `[first inserted, last inserted, ..., second inserted]`.
pub fn link_list_order(inserted: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(inserted.len());
    if let Some((first, rest)) = inserted.split_first() {
        out.push(*first);
        out.extend(rest.iter().rev().copied());
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// The assembler
// ---------------------------------------------------------------------------------------------------

/// Instances and cells assigned to one chunk.
#[derive(Default)]
struct ChunkObjects {
    /// Instances in load order (`mesh` is patched when the chunk is built).
    instances: Vec<StaticInstance>,
    /// Distinct meshes by `model_index` (the `SMSH` order).
    meshes: BTreeMap<u32, Arc<CollisionMesh>>,
    /// `(level, cell_z, cell_x)` to the instance indices in insertion order.
    cells: BTreeMap<(u8, u16, u16), Vec<u32>>,
}

/// The object pass of a world (instances, link cells, owning chunks) over which single chunks can be
/// built, so a caller can assemble and encode lands one at a time and in parallel. Created by
/// [`WorldAssembly::new`]; `assemble_world` is `new` plus [`WorldAssembly::chunk`] for every land.
pub struct WorldAssembly<'a> {
    inp: &'a CollisionInputs,
    /// Indexed `z * lands_w + x`.
    objects: Vec<Option<ChunkObjects>>,
}

impl<'a> WorldAssembly<'a> {
    /// Runs the object pass over **every** land's object list (cross-land link cells need all of them).
    /// Object counters go into `report`.
    pub fn new(inp: &'a CollisionInputs, report: &mut CollisionReport) -> WorldAssembly<'a> {
        let n = usize::from(inp.lands_w) * usize::from(inp.lands_h);
        let mut objects: Vec<Option<ChunkObjects>> = Vec::new();
        objects.resize_with(n, || None);
        let mut me = WorldAssembly { inp, objects };
        for land in &inp.lands {
            me.place_land(land, report);
        }
        me
    }

    fn land_index(&self, x: u16, z: u16) -> Option<usize> {
        if x < self.inp.lands_w && z < self.inp.lands_h {
            Some(usize::from(z) * usize::from(self.inp.lands_w) + usize::from(x))
        } else {
            None
        }
    }

    fn place_land(&mut self, land: &LandInput, report: &mut CollisionReport) {
        let inp = self.inp;
        let mpu = u32::from(inp.mpu);
        let land_span = i64::from(land.x) * 128 * i64::from(mpu);
        let land_span_z = i64::from(land.z) * 128 * i64::from(mpu);
        for o in &land.objects {
            report.objects_read += 1;
            if o.obj_type != OT_OBJ {
                report.dropped_not_obj += 1;
                continue;
            }
            if o.model_index <= 10 {
                report.random_angle_candidates += 1;
            }
            let Some(mesh) = &o.mesh else {
                report.dropped_no_mesh += 1;
                continue;
            };
            let finite = o
                .pos_file
                .iter()
                .chain(&o.angles)
                .chain(&o.scale)
                .all(|v| v.is_finite());
            if !finite {
                report.dropped_non_finite += 1;
                continue;
            }
            // Obj::Read: x, z *= OLD_MPU (f32); ReadWorld: += x * MAP_SIZE * MPU (int, then to float).
            let pos = Vec3::new(
                o.pos_file[0] * OLD_MPU + land_span as f32,
                o.pos_file[1],
                o.pos_file[2] * OLD_MPU + land_span_z as f32,
            );
            let scale = Vec3::new(o.scale[0], o.scale[1], o.scale[2]);
            let (world, axes) = instance_matrices(pos, o.angles, scale);
            let Some(inv_world) = invert_f64(&world) else {
                report.dropped_singular += 1;
                continue;
            };
            let amin = Vec3::new(mesh.aabb_min[0], mesh.aabb_min[1], mesh.aabb_min[2]);
            let amax = Vec3::new(mesh.aabb_max[0], mesh.aabb_max[1], mesh.aabb_max[2]);
            let Some((level, cell_x, cell_z)) =
                link_placement(amin, amax, &world, pos, mpu, u32::from(inp.lands_w))
            else {
                report.dropped_unlinked += 1;
                continue;
            };
            let width = u32::try_from(link_width(usize::from(level), mpu)).unwrap_or(0);
            let (Some(owner_x), Some(owner_z)) = (
                cell_x
                    .checked_div(width)
                    .and_then(|v| u16::try_from(v).ok()),
                cell_z
                    .checked_div(width)
                    .and_then(|v| u16::try_from(v).ok()),
            ) else {
                report.dropped_unlinked += 1;
                continue;
            };
            let (Some(owner), Ok(cx), Ok(cz)) = (
                self.land_index(owner_x, owner_z),
                u16::try_from(cell_x),
                u16::try_from(cell_z),
            ) else {
                report.dropped_unlinked += 1;
                continue;
            };
            // OBB (rule 4): centre = ((min + max) * 0.5) transformed by `world`, extents scaled.
            let center = canon3(transform_coord((amin + amax) * 0.5, &world));
            let half = (amax - amin) * 0.5;
            let inst = StaticInstance {
                mesh: 0, // patched when the chunk is built
                model_index: mesh.model_index,
                src_land_x: land.x,
                src_land_z: land.z,
                src_ordinal: o.ordinal,
                pos,
                angles: o.angles,
                scale,
                link_level: level,
                flags: o.flags & (INSTANCE_FLAG_ALWAYS_OBB | INSTANCE_FLAG_TRANS),
                world,
                inv_world,
                obb: Obb {
                    center,
                    extent: [
                        canon(half.x * scale.x),
                        canon(half.y * scale.y),
                        canon(half.z * scale.z),
                    ],
                    axis: axes,
                },
            };
            report.instances += 1;
            report.triangles += mesh.tris.len() as u64;
            if (owner_x, owner_z) != (land.x, land.z) {
                report.cross_land_instances += 1;
            }
            let Some(slot) = self.objects.get_mut(owner) else {
                continue;
            };
            let chunk = slot.get_or_insert_with(ChunkObjects::default);
            chunk
                .meshes
                .entry(mesh.model_index)
                .or_insert_with(|| Arc::clone(mesh));
            let index = u32::try_from(chunk.instances.len()).unwrap_or(u32::MAX);
            chunk.instances.push(inst);
            chunk.cells.entry((level, cz, cx)).or_default().push(index);
        }
    }

    /// The raw height of vertex `(vx, vz)` of land `(x, z)` under the server heightmap view
    /// (§10.7.2 vertex semantics).
    fn raw_vertex(
        &self,
        own: Option<&LandInput>,
        x: u16,
        z: u16,
        vx: usize,
        vz: usize,
        unavailable: &mut bool,
    ) -> f32 {
        let own_edge = vx == MAP_SIZE || vz == MAP_SIZE;
        // The land that owns the vertex in the server's global array, and the vertex inside it.
        let (tx, tz, lx, lz) = if own_edge {
            (
                u32::from(x) + u32::from(vx == MAP_SIZE),
                u32::from(z) + u32::from(vz == MAP_SIZE),
                if vx == MAP_SIZE { 0 } else { vx },
                if vz == MAP_SIZE { 0 } else { vz },
            )
        } else {
            (u32::from(x), u32::from(z), vx, vz)
        };
        let inside = tx < u32::from(self.inp.lands_w) && tz < u32::from(self.inp.lands_h);
        let (land, ix, iz) = if inside {
            let l = u16::try_from(tx)
                .ok()
                .zip(u16::try_from(tz).ok())
                .and_then(|(a, b)| self.inp.land(a, b));
            (l, lx, lz)
        } else {
            // Outside the world: the land's own column/row 128 (never read by the rules).
            (own, vx, vz)
        };
        let Some(land) = land.filter(|l| l.present) else {
            return 0.0; // no file: the server's memset(0)
        };
        match land
            .heights_raw
            .as_deref()
            .and_then(|h| h.get(iz * VERTS_PER_SIDE + ix))
        {
            Some(v) => *v,
            None => {
                *unavailable = true;
                0.0
            }
        }
    }

    /// Assembles the chunk of land `(x, z)`; `None` when the land is outside the world. Terrain counters
    /// and the per-chunk object counters go into `report`.
    pub fn chunk(&self, x: u16, z: u16, report: &mut CollisionReport) -> Option<LandChunk> {
        let idx = self.land_index(x, z)?;
        let own = self.inp.land(x, z);
        let present = own.is_some_and(|l| l.present);
        let mut c = LandChunk::empty(
            x,
            z,
            self.inp.lands_w,
            self.inp.lands_h,
            self.inp.mpu,
            present,
        );
        report.chunks += 1;
        if present {
            report.lands_present += 1;
        } else {
            report.lands_missing += 1;
        }

        // Terrain.
        let mut heights = Vec::with_capacity(VERT_COUNT);
        let mut attrs = Vec::with_capacity(VERT_COUNT);
        let mut unavailable = false;
        for vz in 0..VERTS_PER_SIDE {
            for vx in 0..VERTS_PER_SIDE {
                let mut raw = self.raw_vertex(own, x, z, vx, vz, &mut unavailable);
                if !raw.is_finite() {
                    report.nonfinite_heights += 1;
                    raw = 0.0;
                }
                heights.push(strip_height_server(raw));
                attrs.push(HeightAttr::classify(raw) as u8);
            }
        }
        if unavailable {
            report.terrain_unavailable += 1;
        }
        c.heights = Heights::from_values(&heights).unwrap_or(Heights::Uniform(0.0));
        c.attrs = Attrs::from_values(&attrs).unwrap_or(Attrs::Uniform(0));
        match &c.heights {
            Heights::Uniform(_) => report.height_uniform += 1,
            Heights::Q16 { .. } => report.height_q16 += 1,
            Heights::F32(_) => report.height_f32 += 1,
        }
        match &c.attrs {
            Attrs::Uniform(_) => report.attr_uniform += 1,
            Attrs::Dense(_) => report.attr_dense += 1,
        }
        if let Some(l) = own.filter(|l| l.present) {
            c.water = l.water;
        }

        // Objects.
        if let Some(Some(obj)) = self.objects.get(idx) {
            c.meshes = obj.meshes.values().map(|m| (**m).clone()).collect();
            let mesh_pos: BTreeMap<u32, u32> = obj
                .meshes
                .keys()
                .enumerate()
                .map(|(i, k)| (*k, u32::try_from(i).unwrap_or(u32::MAX)))
                .collect();
            c.instances = obj
                .instances
                .iter()
                .map(|i| {
                    let mut i = i.clone();
                    i.mesh = mesh_pos.get(&i.model_index).copied().unwrap_or(0);
                    i
                })
                .collect();
            c.links = obj
                .cells
                .iter()
                .map(|((level, cz, cx), list)| {
                    if list.len() > MAX_CELL_LIST {
                        report.link_cell_overflow += 1;
                    }
                    let shown = list.get(..list.len().min(MAX_CELL_LIST)).unwrap_or(list);
                    LinkCell {
                        level: *level,
                        cell_x: *cx,
                        cell_z: *cz,
                        instances: link_list_order(shown),
                    }
                })
                .collect();
            report.meshes += c.meshes.len() as u64;
            report.mesh_triangles += c.meshes.iter().map(|m| m.tris.len() as u64).sum::<u64>();
            report.link_cells += c.links.len() as u64;
        }
        Some(c)
    }
}

/// Assembles the chunk of every land of `inp`, in server load order (`z` outer, `x` inner)
/// (Plan/24 §10.7; WP-WEB-13). Pure: no I/O, no panics, deterministic. Encode the result with
/// `flyff_sim::encode_chunk`.
pub fn assemble_world(inp: &CollisionInputs, report: &mut CollisionReport) -> Vec<LandChunk> {
    let asm = WorldAssembly::new(inp, report);
    let mut out = Vec::with_capacity(usize::from(inp.lands_w) * usize::from(inp.lands_h));
    for z in 0..inp.lands_h {
        for x in 0..inp.lands_w {
            if let Some(c) = asm.chunk(x, z, report) {
                out.push(c);
            }
        }
    }
    out
}
