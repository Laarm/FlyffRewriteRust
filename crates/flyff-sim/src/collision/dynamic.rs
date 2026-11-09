//! Dynamic `OT_CTRL` colliders: doors, chests, traps, psychic walls (Plan/31 §10.10; F-091 part; WP-SIM-24).
//!
//! The only shape input is the `FLYK` file ([`CtrlModelSet`], Plan/24 §10.8). A collider is one placed ctrl:
//! its world matrix, the OBB of the model box, its animation frame and the **pose** at that frame — for each
//! rigid LOD-0 object `i` the matrix `update[i] · world` and its inverse
//! ([`crate::pose::rigid::animate_objects`]; the deterministic 4x4 inverse of Plan/31 §13.4 is computed once
//! per ctrl per frame change, never per query). A query tests the OBB first (render bounds, no collision-mesh
//! requirement) and then the triangles of the objects in file order, first triangle wins
//! ([`CollisionWorld::process_collision`](super::world::CollisionWorld::process_collision),
//! `CObject3D::SlideVectorXZ2` with `bCollObj = FALSE`).
//!
//! **Candidate order** (FIX, Plan/31 §10.10 rule 3): the original walked the dynamic link map (head insert,
//! then reverse insertion order), which depends on server and client spawn order. Here the walk is the
//! single-level `FOR_LINKMAP` of the original (cell size `ftoi(32 · 0.25 · mpu)` world units = 16 cells per
//! land side, a ctrl's cell is the same linear `nPos` as the static placement, cells z-major) with **ascending
//! `seq`** inside a cell, so any two sides that agree on the `seq` of each ctrl agree on every result. A
//! ctrl whose cell would be outside the table is not linked and never collides (as in the original).
//! [`DynamicColliders::new`] has no grid: it offers every collider in `seq` order (tests, previews).
//!
//! **Ledger (PL-019, FIX):** the original world server never animated `_mUpdate` (identity matrices: the
//! parts collapsed at the model origin) and the client used the shared, last-rendered pose; both roles of
//! the port use the per-instance pose at the ctrl's own frame.
//!
//! **Housing** (`ProcessUnderCollision` in mini-room / guild-house worlds): the original tested the ctrl's
//! *collision body* (`m_CollObject`) there; `FLYK` carries the LOD-0 render objects only, so those worlds
//! (M7) use the render meshes of the ctrl (a documented simplification, `NOTES.md`).

use super::chunk::{CollisionMesh, Obb};
use super::ctrl_models::CtrlModelSet;
use super::geom::Aabb;
use super::query::MeshRef;
use crate::num::{Vec3, ftoi_x86, sqrtf};
use crate::pose::quat::{inverse4x4, mul_affine};
use crate::pose::rigid::animate_objects;
use core::ops::ControlFlow;
use flyff_common::ObjId;
use flyff_common::math::D3dMatrix;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The dynamic link cell size in world units at MPU 1 (`g_table[0] · 0.25` of the original at MPU 4: 8 cells
/// of the height map).
const DYN_CELL_FACTOR: f32 = 32.0 * 0.25;
/// Heightmap cells per land side (`MAP_SIZE`).
const MAP_SIZE: i32 = 128;

/// The grid of the dynamic link map of a world (`CLinkMap::Init` for `linkDynamic`, level 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DynGrid {
    mpu: i32,
    lands_w: i32,
    lands_h: i32,
}

impl DynGrid {
    /// Link cells per land side.
    fn width(self) -> i32 {
        let cell = ftoi_x86(DYN_CELL_FACTOR * self.mpu as f32); // BOUNDS: mpu is 1..=4
        if cell <= 0 {
            return 0;
        }
        (MAP_SIZE * self.mpu) / cell
    }

    /// `CLinkMap::GetObjPtr` for level 0: the linear `nPos`, valid iff `0 <= nPos < nMaxWidth^2`.
    fn cell_of(self, pos: Vec3) -> Option<(i32, i32)> {
        let width = self.width();
        if width <= 0 || self.lands_w <= 0 {
            return None;
        }
        let max_w = width.checked_mul(self.lands_w)?;
        let unit = (MAP_SIZE.checked_mul(self.lands_w)?) / max_w;
        if unit <= 0 {
            return None;
        }
        let mpu_f = self.mpu as f32;
        let unit_f = unit as f32;
        let nz = ftoi_x86(pos.z / mpu_f / unit_f);
        let nx = ftoi_x86(pos.x / mpu_f / unit_f);
        let n_pos = nz.wrapping_mul(max_w).wrapping_add(nx);
        let limit = max_w.checked_mul(max_w)?;
        if n_pos < 0 || n_pos >= limit {
            return None;
        }
        Some((n_pos / max_w, n_pos % max_w)) // (cell_z, cell_x)
    }
}

/// One rigid object's pose: `update[i] · world` and its inverse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartPose {
    /// Object -> world.
    pub world: D3dMatrix,
    /// World -> object.
    pub inv_world: D3dMatrix,
}

/// One placed ctrl collider.
#[derive(Clone, Debug)]
pub struct CtrlCollider {
    /// The ctrl's object id.
    pub id: ObjId,
    /// `mdlDyna` ctrl model index (`CtrlModel::ctrl_index`).
    pub model: u32,
    /// Spawn order: the candidate order inside a link cell is ascending `seq`.
    pub seq: u64,
    /// World matrix of the ctrl (`S · R · T`).
    pub world: D3dMatrix,
    /// World-space OBB of the model box (render bounds; no collision-mesh requirement).
    pub obb: Obb,
    /// The model box (object space).
    pub aabb: Aabb,
    /// The animation frame the pose was evaluated at.
    pub frame: f32,
    /// The `GetNextFrame` the pose was evaluated with.
    pub next: i32,
    parts: Vec<PartPose>,
    cell: Option<(i32, i32)>,
}

impl CtrlCollider {
    /// The pose of every rigid object (file order).
    pub fn parts(&self) -> &[PartPose] {
        &self.parts
    }
}

/// What [`DynamicColliders::add`] was given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AddCollider {
    /// The ctrl's object id (an existing collider of this id is replaced).
    pub id: ObjId,
    /// `mdlDyna` ctrl index.
    pub model: u32,
    /// World matrix.
    pub world: D3dMatrix,
    /// The model box for the OBB; `None` = the box of the `FLYK` model.
    pub aabb: Option<Aabb>,
    /// Spawn order.
    pub seq: u64,
    /// Initial animation frame.
    pub frame: f32,
    /// `GetNextFrame` of that frame.
    pub next: i32,
}

/// The result of [`DynamicColliders::add`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// Registered; `linked` is `false` when the position has no link cell (the ctrl never collides).
    Added {
        /// The collider is in the dynamic link map.
        linked: bool,
    },
    /// The model is not in the `FLYK` set: no collider (counted by [`DynamicColliders::without_model`]).
    NoModel,
}

/// The dynamic colliders of one layer.
#[derive(Clone, Debug, Default)]
pub struct DynamicColliders {
    models: Arc<CtrlModelSet>,
    grid: Option<DynGrid>,
    colliders: BTreeMap<(u64, ObjId), CtrlCollider>,
    by_id: BTreeMap<ObjId, u64>,
    cells: BTreeMap<(i32, i32), Vec<(u64, ObjId)>>,
    without_model: u32,
}

/// The LH world matrix (`Mat43`, `S · RotYawPitchRoll(-yaw) · T`, [`crate::client::view::world_matrix_lh`]) of a
/// ctrl as the wire describes it: `angle_x10` and `scale_x100` are the quantised `ObjCommon` values of
/// `SpawnCtrl` (`(i16)(angle * 10)`, `(u16)(scale * 100)`). The server registers its collider with exactly this
/// matrix and the client builds the same one from the message, so both sides collide with the same ctrl pose.
pub fn ctrl_world_mat43(pos: Vec3, angle_x10: i32, scale_x100: u32) -> [f32; 12] {
    let yaw = angle_x10 as f32 / 10.0; // BOUNDS: an i16 range value
    let scale = scale_x100 as f32 / 100.0; // BOUNDS: a u16 range value
    crate::client::view::world_matrix_lh(pos, yaw, 0.0, scale)
}

/// The world-space OBB of a model box under `world = S · R · T` (`BBOX::UpdateMartix`): centre = the
/// transformed box centre, half extents scaled by the row lengths, axis `i` = row `i` of the rotation.
pub fn obb_of(world: &D3dMatrix, aabb: &Aabb) -> Obb {
    let m = |i: usize| world.0.get(i).copied().unwrap_or(0.0);
    let row = |r: usize| Vec3::new(m(r * 4), m(r * 4 + 1), m(r * 4 + 2));
    let len = |v: Vec3| sqrtf(v.x * v.x + v.y * v.y + v.z * v.z);
    let center_local = (aabb.min + aabb.max) * 0.5;
    let center = crate::collision::geom::transform_coord(center_local, world);
    let half = (aabb.max - aabb.min) * 0.5;
    let (r0, r1, r2) = (row(0), row(1), row(2));
    let (s0, s1, s2) = (len(r0), len(r1), len(r2));
    let unit = |r: Vec3, s: f32| if s > 0.0 { r * (1.0 / s) } else { Vec3::ZERO };
    Obb {
        center,
        extent: [half.x * s0, half.y * s1, half.z * s2],
        axis: [unit(r0, s0), unit(r1, s1), unit(r2, s2)],
    }
}

impl DynamicColliders {
    /// An empty set without a link grid and without models (every collider is a candidate, in `seq` order).
    pub fn new() -> DynamicColliders {
        DynamicColliders::default()
    }

    /// An empty set with the dynamic link grid of a world (`mpu` 1..=4, size in lands).
    pub fn for_world(mpu: u32, lands_w: u32, lands_h: u32) -> DynamicColliders {
        DynamicColliders {
            grid: Some(DynGrid {
                mpu: i32::try_from(mpu).unwrap_or(4),
                lands_w: i32::try_from(lands_w).unwrap_or(1),
                lands_h: i32::try_from(lands_h).unwrap_or(1),
            }),
            ..DynamicColliders::default()
        }
    }

    /// Installs the `FLYK` models. Colliders already registered are re-posed against the new set (those whose
    /// model disappeared are dropped).
    pub fn set_models(&mut self, models: Arc<CtrlModelSet>) {
        self.models = models;
        let ids: Vec<ObjId> = self.colliders.values().map(|c| c.id).collect();
        for id in ids {
            let key = self.by_id.get(&id).map(|&s| (s, id));
            let Some(key) = key else { continue };
            let Some(c) = self.colliders.get(&key).cloned() else {
                continue;
            };
            self.remove(id);
            self.add(AddCollider {
                id,
                model: c.model,
                world: c.world,
                aabb: Some(c.aabb),
                seq: c.seq,
                frame: c.frame,
                next: c.next,
            });
        }
    }

    /// The installed `FLYK` models.
    pub fn models(&self) -> &Arc<CtrlModelSet> {
        &self.models
    }

    /// Registers a collider (replacing one of the same id) and evaluates its pose.
    pub fn add(&mut self, a: AddCollider) -> AddOutcome {
        self.remove(a.id);
        let Some(model) = self.models.get(a.model) else {
            self.without_model = self.without_model.saturating_add(1);
            return AddOutcome::NoModel;
        };
        let aabb = a.aabb.unwrap_or(model.aabb);
        let pos = Vec3::new(
            a.world.0.get(12).copied().unwrap_or(0.0),
            a.world.0.get(13).copied().unwrap_or(0.0),
            a.world.0.get(14).copied().unwrap_or(0.0),
        );
        let cell = match self.grid {
            Some(g) => g.cell_of(pos),
            None => Some((0, 0)),
        };
        let mut c = CtrlCollider {
            id: a.id,
            model: a.model,
            seq: a.seq,
            world: a.world,
            obb: obb_of(&a.world, &aabb),
            aabb,
            frame: a.frame,
            next: a.next,
            parts: Vec::new(),
            cell,
        };
        c.parts = pose_parts(model, &a.world, a.frame, a.next);
        let key = (a.seq, a.id);
        if let Some(cell) = cell
            && self.grid.is_some()
        {
            self.cells.entry(cell).or_default().push(key);
            if let Some(v) = self.cells.get_mut(&cell) {
                v.sort_unstable();
            }
        }
        self.by_id.insert(a.id, a.seq);
        let linked = cell.is_some();
        self.colliders.insert(key, c);
        AddOutcome::Added { linked }
    }

    /// Re-evaluates the pose of ctrl `id` at `frame` (`next` = its clock's `GetNextFrame`); `false` when the
    /// ctrl has no collider. A call with the frame pair of the last evaluation does nothing.
    pub fn set_pose(&mut self, id: ObjId, frame: f32, next: i32) -> bool {
        let Some(&seq) = self.by_id.get(&id) else {
            return false;
        };
        let models = Arc::clone(&self.models);
        let Some(c) = self.colliders.get_mut(&(seq, id)) else {
            return false;
        };
        if c.frame == frame && c.next == next && !c.parts.is_empty() {
            return true;
        }
        c.frame = frame;
        c.next = next;
        if let Some(model) = models.get(c.model) {
            c.parts = pose_parts(model, &c.world, frame, next);
        }
        true
    }

    /// Moves ctrl `id` to a new world matrix (housing furniture): the OBB, the pose and the link cell follow.
    pub fn set_world(&mut self, id: ObjId, world: D3dMatrix) -> bool {
        let Some(&seq) = self.by_id.get(&id) else {
            return false;
        };
        let Some(c) = self.colliders.get(&(seq, id)).cloned() else {
            return false;
        };
        self.add(AddCollider {
            id,
            model: c.model,
            world,
            aabb: Some(c.aabb),
            seq,
            frame: c.frame,
            next: c.next,
        });
        true
    }

    /// Removes the collider of ctrl `id`; returns whether it existed.
    pub fn remove(&mut self, id: ObjId) -> bool {
        let Some(seq) = self.by_id.remove(&id) else {
            return false;
        };
        let key = (seq, id);
        if let Some(c) = self.colliders.remove(&key)
            && let Some(cell) = c.cell
        {
            let empty = self.cells.get_mut(&cell).is_some_and(|v| {
                v.retain(|k| *k != key);
                v.is_empty()
            });
            if empty {
                self.cells.remove(&cell);
            }
        }
        true
    }

    /// The collider of ctrl `id`.
    pub fn get(&self, id: ObjId) -> Option<&CtrlCollider> {
        let seq = self.by_id.get(&id)?;
        self.colliders.get(&(*seq, id))
    }

    /// Number of colliders.
    pub fn len(&self) -> usize {
        self.colliders.len()
    }

    /// `true` when there are no colliders.
    pub fn is_empty(&self) -> bool {
        self.colliders.is_empty()
    }

    /// Number of `add` calls whose model was missing from the `FLYK` set (never collide).
    pub fn without_model(&self) -> u32 {
        self.without_model
    }

    /// The colliders in ascending `(seq, id)` order.
    pub fn iter(&self) -> impl Iterator<Item = &CtrlCollider> {
        self.colliders.values()
    }

    /// The triangle meshes of `c` with their current pose, in object (file) order.
    pub fn meshes<'a>(&'a self, c: &'a CtrlCollider) -> impl Iterator<Item = MeshRef<'a>> + 'a {
        let objects: &'a [super::ctrl_models::RigidObject] = self
            .models
            .get(c.model)
            .map(|m| m.objects.as_slice())
            .unwrap_or(&[]);
        objects.iter().zip(c.parts.iter()).map(|(o, p)| MeshRef {
            mesh: &o.mesh,
            world: &p.world,
            inv_world: &p.inv_world,
        })
    }

    /// The mesh of rigid object `i` of `c` (file order) with its current pose.
    pub fn mesh_of<'a>(
        &'a self,
        c: &'a CtrlCollider,
        i: usize,
    ) -> Option<(&'a CollisionMesh, &'a PartPose)> {
        let m = self.models.get(c.model)?;
        Some((&m.objects.get(i)?.mesh, c.parts.get(i)?))
    }

    /// `FOR_LINKMAP(world, pos, obj, 0, linkDynamic)`: visits the colliders of the 2x2 cell neighbourhood of
    /// `pos` z-major, ascending `seq` inside a cell (every collider in `seq` order without a grid); `f` returns
    /// `ControlFlow::Break` to stop.
    pub fn walk<F>(&self, pos: Vec3, mut f: F)
    where
        F: FnMut(&CtrlCollider) -> ControlFlow<()>,
    {
        if self.colliders.is_empty() {
            return;
        }
        let Some(g) = self.grid else {
            for c in self.colliders.values() {
                if f(c).is_break() {
                    return;
                }
            }
            return;
        };
        let width = g.width();
        if width <= 0 {
            return;
        }
        let max_w = width.saturating_mul(g.lands_w);
        let max_h = width.saturating_mul(g.lands_h);
        let unit = MAP_SIZE / width;
        if unit <= 0 {
            return;
        }
        let mpu_f = g.mpu as f32;
        let lx = ftoi_x86(pos.x / mpu_f);
        let lz = ftoi_x86(pos.z / mpu_f);
        let px = ftoi_x86(pos.x);
        let pz = ftoi_x86(pos.z);
        let ox = (lx / unit) * unit * g.mpu;
        let oz = (lz / unit) * unit * g.mpu;
        let d = unit * g.mpu / 2;
        let bx = i32::from(px - ox > d);
        let bz = i32::from(pz - oz > d);
        let x0 = ((lx / unit) + (bx - 1)).max(0);
        let z0 = ((lz / unit) + (bz - 1)).max(0);
        let x1 = ((lx / unit) + bx).min(max_w - 1);
        let z1 = ((lz / unit) + bz).min(max_h - 1);
        let mut cz = z0;
        while cz <= z1 {
            let mut cx = x0;
            while cx <= x1 {
                if let Some(keys) = self.cells.get(&(cz, cx)) {
                    for key in keys.iter().take(super::linkmap::LINK_CELL_LIMIT) {
                        if let Some(c) = self.colliders.get(key)
                            && f(c).is_break()
                        {
                            return;
                        }
                    }
                }
                cx += 1;
            }
            cz += 1;
        }
    }
}

/// `update[i] · world` and its inverse for every rigid object of `model` at `frame`.
fn pose_parts(
    model: &super::ctrl_models::CtrlModel,
    world: &D3dMatrix,
    frame: f32,
    next: i32,
) -> Vec<PartPose> {
    let mut updates = Vec::with_capacity(model.objects.len());
    animate_objects(&model.objects, frame, next, &mut updates);
    updates
        .iter()
        .map(|u| {
            let w = mul_affine(u, world);
            PartPose {
                inv_world: inverse4x4(&w),
                world: w,
            }
        })
        .collect()
}
