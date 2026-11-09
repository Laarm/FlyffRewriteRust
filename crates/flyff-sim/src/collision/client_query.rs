//! Client-side collision utilities (Plan/31 §10.11): the ray cast with normals used for picking and the
//! client line-of-sight refusal, and the camera collision test `camera_check_bound` (Plan/43 §8.4,
//! WP-INPUT-03), and the Blinkpool ground point `blink_point` (Plan/43 §7.10, WP-INPUT-13; `target_check` is
//! `ClientSim::target_check`, `range_point` is `bounds::is_range_point` over a mover, see
//! `ClientSim::range_point`).

use super::chunk::INSTANCE_FLAG_TRANS;
use super::dynamic::DynamicColliders;
use super::geom::ray_tri;
use super::geom::{Segment3, obb_line_test, seg_obb_test, transform_coord, transform_normal};
use super::query::{MeshRef, find_touch_tri_line};
use super::terrain::QueryCtx;
use super::world::{CollisionWorld, LandSlot};
use crate::num::{Vec3, cross, length, length_sq, normalize_or_zero};
use crate::types::{HATTR_NOFLY, HATTR_NONE};
use core::ops::ControlFlow;
use flyff_common::math::ftoi_x86;

/// `ray_cast` mask bit: terrain triangles.
pub const RAY_TERRAIN: u32 = 1;
/// `ray_cast` mask bit: static collision meshes.
pub const RAY_STATIC: u32 = 2;
/// `ray_cast` mask bit: ctrl colliders.
pub const RAY_CTRL: u32 = 4;
/// `RayHit::kind`: nothing.
pub const RAY_KIND_NONE: u8 = 0;
/// `RayHit::kind`: terrain (`id` = land index `lx + lz * lands_w`).
pub const RAY_KIND_TERRAIN: u8 = 1;
/// `RayHit::kind`: static object (`id` = `land_index * 65536 + sobj_index`).
pub const RAY_KIND_STATIC: u8 = 2;
/// `RayHit::kind`: ctrl (`id` = its `ObjId`).
pub const RAY_KIND_CTRL: u8 = 3;

/// `WTYPE_WATER` (`landscape.h:42`): the water type of a patch is `texture & 3`.
pub const WTYPE_WATER: u8 = 2;
/// `~MASK_WATERFRAME` (`MASK_WATERFRAME = 0xfc`): the bits of a water texture byte that hold the type.
pub const WATER_TYPE_MASK: u8 = 0x03;
/// Terrain clearance of the camera (`__FIX_PICKING`, `World3D.cpp:1700`): was `0.3` before version 11.
pub const CAMERA_TERRAIN_CLEARANCE: f32 = 0.6;
/// Water clearance of the camera (`World3D.cpp:1722`).
pub const CAMERA_WATER_CLEARANCE: f32 = 0.3;
/// `nRange` of `IntersectObjLine2` (the link-map walk radius around the look-at).
pub const CAMERA_LINK_RANGE: i32 = 5;

/// The result of [`CollisionWorld::camera_check_bound`] (`CWorld::CheckBound(vPos, vDest, &vOut, &fLength)`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CameraBound {
    /// Something is between the look-at and the camera.
    pub crash: bool,
    /// The camera position to use: the hit point, or the requested position without a hit.
    pub out: Vec3,
    /// Distance from the look-at to `out` (the requested distance without a hit).
    pub len: f32,
}

/// The most 1 m steps [`CollisionWorld::blink_point`] walks (the original ends by its range test; a hostile range
/// value must not turn the loop into a long one).
pub const BLINK_MAX_STEPS: u32 = 4096;

/// The result of [`CollisionWorld::blink_point`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BlinkPoint {
    /// The teleport point.
    pub pos: Vec3,
    /// `false`: the search found no usable point and fell back to the caster's own position.
    pub ok: bool,
}

/// The nearest hit of [`CollisionWorld::ray_cast`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RayHit {
    /// Ray parameter in units of the direction (the distance for a unit direction).
    pub t: f32,
    /// `origin + dir * t`.
    pub pos: Vec3,
    /// `RAY_KIND_*`.
    pub kind: u8,
    /// Terrain: land index; static: `land_index * 65536 + sobj_index`; ctrl: object id.
    pub id: u32,
    /// Unit normal of the hit triangle in its stored winding (terrain: the `land_tri` order; objects: the
    /// object-space normal transformed by the linear part of the world matrix and re-normalised).
    pub normal: Vec3,
}

impl CollisionWorld {
    /// `ray_cast(origin, dir, max_t, mask)` (Plan/31 §10.11): the nearest hit with `t` in `[0, max_t]`
    /// over the enabled categories. Terrain is traversed with a 2-D DDA over the heightmap cells and both
    /// triangle windings are tested; static and ctrl meshes are back-face culled like every other query.
    /// Pass a unit `dir` to get `t` as a distance. `Pending` lands have no terrain.
    pub fn ray_cast(
        &self,
        dynamic: &DynamicColliders,
        origin: Vec3,
        dir: Vec3,
        max_t: f32,
        mask: u32,
    ) -> Option<RayHit> {
        if !max_t.is_finite() || max_t < 0.0 || dir == Vec3::ZERO {
            return None;
        }
        if !origin.x.is_finite() || !origin.y.is_finite() || !origin.z.is_finite() {
            return None;
        }
        let mut best: Option<RayHit> = None;
        let mut consider = |h: RayHit| {
            if best.is_none_or(|b| h.t < b.t) {
                best = Some(h);
            }
        };
        if mask & RAY_TERRAIN != 0
            && let Some(h) = self.ray_terrain(origin, dir, max_t)
        {
            consider(h);
        }
        if mask & RAY_STATIC != 0
            && let Some(h) = self.ray_statics(origin, dir, max_t)
        {
            consider(h);
        }
        if mask & RAY_CTRL != 0
            && let Some(h) = ray_ctrls(dynamic, origin, dir, max_t)
        {
            consider(h);
        }
        best
    }

    /// `CWorld::CheckBound(cam, look_at)` (Plan/43 §8.4, `World3D.cpp:1670-1790`): the camera collision.
    ///
    /// 1. A terrain march from `look_at + 1` unit towards the camera in 0.1 steps, `floor(length * 10)`
    ///    steps (KEEP-QUIRK: the march starts one unit out and so ends about one unit PAST the camera):
    ///    a sample hits when `y < land_height + 0.6` (`__FIX_PICKING`; was 0.3), and, when the look-at lies
    ///    above a `WTYPE_WATER` patch, when `y < water_height + 0.3`.
    /// 2. The nearest static collision-mesh hit of the segment `look_at -> cam` over the link-map cells
    ///    around the look-at (range 5), skipping instances flagged `TRANS` (FIX: the original overwrote the
    ///    segment direction with the end point after the first hit, so later hits were tested against a
    ///    wrong segment; every instance is tested against the same segment here).
    /// 3. The hit nearer to the look-at wins; without a hit `out = cam` and `len = |cam - look_at|`.
    pub fn camera_check_bound(&self, cam: Vec3, look_at: Vec3) -> CameraBound {
        let diff = cam - look_at;
        let dist = length(diff);
        let n = normalize_or_zero(diff);
        // `tempVec /= 10` (a division of each component, not a multiplication by 0.1)
        let step = Vec3::new(n.x / 10.0, n.y / 10.0, n.z / 10.0);
        let mut cur = look_at + step * 10.0;
        let water_height = self.water_at(look_at).and_then(|(h, tex)| {
            if tex & WATER_TYPE_MASK == WTYPE_WATER {
                Some(f32::from(h))
            } else {
                None
            }
        });
        // the look-at above the water surface: the camera must not dip into it
        let water_dn = water_height.filter(|&h| h < look_at.y);
        let mut hit1: Option<Vec3> = None;
        let steps = ftoi_x86(dist * 10.0).max(0);
        for _ in 0..steps {
            cur += step;
            let ground = self.land_height(cur.x, cur.z);
            if cur.y < ground + CAMERA_TERRAIN_CLEARANCE {
                hit1 = Some(cur);
                break;
            }
            if let Some(h) = water_dn
                && cur.y < h + CAMERA_WATER_CLEARANCE
            {
                hit1 = Some(cur);
                break;
            }
        }
        let hit2 = self.camera_static_hit(look_at, cam);
        let pick = match (hit1, hit2) {
            (Some(a), Some(b)) => {
                if length_sq(a - look_at) < length_sq(b - look_at) {
                    Some(a)
                } else {
                    Some(b)
                }
            }
            (a, b) => a.or(b),
        };
        match pick {
            Some(out) => CameraBound {
                crash: true,
                out,
                len: length(out - look_at),
            },
            None => CameraBound {
                crash: false,
                out: cam,
                len: dist,
            },
        }
    }

    /// The Blinkpool ground point (`CMover::CMD_SetUseSkill`, `WUI_TARGETCURSORPTZ`, `MoverMsg.cpp:240-338`; Plan/43
    /// §7.10, WP-INPUT-13): `from` is the caster, `to` the ground point under the cursor, `range` the
    /// `skill_range` of the skill level.
    ///
    /// * Farther than `range`: the point is clamped to `from + dir * range` (keeping the cursor's `y`). When its
    ///   height attribute is not `HATTR_NONE` the search walks back along the same line in 1 m steps until a
    ///   `NONE` cell is found, stopping at a `HATTR_NOFLY` cell; one more step is taken and, when that cell is
    ///   not `NONE` either, the result is the caster's own position.
    /// * Within `range`: the cursor point stands as long as its cell is `NONE`. KEEP-QUIRK: when it is not, the
    ///   original restarts at length 0 and steps to `from - dir * 1`, `from - dir * 2`, ... (BEHIND the caster,
    ///   not back from the target), stopping at a `NOFLY` cell.
    /// * Either walk ends at the caster's own position (`ok == false`) as soon as a candidate lies farther than
    ///   `range` from the caster.
    ///
    /// A non-finite input answers the caster's position with `ok == false`.
    pub fn blink_point(&self, from: Vec3, to: Vec3, range: f32) -> BlinkPoint {
        let own = BlinkPoint {
            pos: from,
            ok: false,
        };
        let finite = |p: Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
        if !finite(from) || !finite(to) || !range.is_finite() {
            return own;
        }
        let max_sq = range * range;
        let vdist = to - from;
        let dir = normalize_or_zero(vdist);
        let attr_at = |p: Vec3| self.height_attr(p.x, p.z);
        // `fTemp > fMaxDistSq` after a candidate: the search gave up
        let too_far = |p: Vec3| length_sq(p - from) > max_sq;
        let mut pos = to;
        if length_sq(vdist) > max_sq {
            let mut len = range;
            let y = to.y;
            pos = from + dir * len;
            pos.y = y;
            let mut attr = attr_at(pos);
            if attr != HATTR_NONE {
                let mut steps = 0;
                while attr != HATTR_NONE {
                    if attr == HATTR_NOFLY {
                        break;
                    }
                    len -= 1.0;
                    pos = from + dir * len;
                    attr = attr_at(pos);
                    if too_far(pos) {
                        // the original sets the caster's position here, but the step below overwrites it
                        break;
                    }
                    steps += 1;
                    if steps >= BLINK_MAX_STEPS {
                        return own;
                    }
                }
                // one more metre back (the original "once more"), then the cell must be walkable
                len -= 1.0;
                pos = from + dir * len;
                if attr_at(pos) != HATTR_NONE {
                    pos = from;
                }
            }
        } else {
            let mut attr = attr_at(pos);
            let mut len = 0.0_f32;
            let mut steps = 0;
            while attr != HATTR_NONE {
                if attr == HATTR_NOFLY {
                    break;
                }
                len -= 1.0;
                pos = from + dir * len;
                attr = attr_at(pos);
                if too_far(pos) {
                    pos = from;
                    break;
                }
                steps += 1;
                if steps >= BLINK_MAX_STEPS {
                    return own;
                }
            }
        }
        BlinkPoint {
            pos,
            ok: pos != from,
        }
    }

    /// The static part of [`CollisionWorld::camera_check_bound`]: the hit point nearest to `a` on the segment
    /// `a -> b` (`IntersectObjLine2(.., bSkipTrans = TRUE)`).
    fn camera_static_hit(&self, a: Vec3, b: Vec3) -> Option<Vec3> {
        let mut best: Option<(f32, Vec3)> = None;
        let d = b - a;
        self.for_linkmap(a, CAMERA_LINK_RANGE, |r| {
            let (Some(inst), Some(mesh)) = (r.instance(), r.mesh()) else {
                return ControlFlow::Continue(());
            };
            if inst.flags & INSTANCE_FLAG_TRANS != 0 || !obb_line_test(inst, a, b) {
                return ControlFlow::Continue(());
            }
            let m = MeshRef {
                mesh,
                world: &inst.world,
                inv_world: &inst.inv_world,
            };
            if let Some((_, t)) = find_touch_tri_line(m, a, b)
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, a + d * t));
            }
            ControlFlow::Continue(())
        });
        best.map(|(_, p)| p)
    }

    fn ray_terrain(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<RayHit> {
        let mpu = self.mpu as f32;
        let (u, v) = (origin.x / mpu, origin.z / mpu);
        let (du, dv) = (dir.x / mpu, dir.z / mpu);
        let mut cx = ftoi_x86(u.floor());
        let mut cz = ftoi_x86(v.floor());
        let step_x = if du > 0.0 { 1 } else { -1 };
        let step_z = if dv > 0.0 { 1 } else { -1 };
        let next_x = if du > 0.0 { cx as f32 + 1.0 } else { cx as f32 };
        let next_z = if dv > 0.0 { cz as f32 + 1.0 } else { cz as f32 };
        let mut t_max_x = if du != 0.0 {
            (next_x - u) / du
        } else {
            f32::INFINITY
        };
        let mut t_max_z = if dv != 0.0 {
            (next_z - v) / dv
        } else {
            f32::INFINITY
        };
        let t_delta_x = if du != 0.0 {
            1.0 / du.abs()
        } else {
            f32::INFINITY
        };
        let t_delta_z = if dv != 0.0 {
            1.0 / dv.abs()
        } else {
            f32::INFINITY
        };
        let w = self.world_width_cells();
        let h = self.world_height_cells();
        let max_cells = (du.abs() + dv.abs()) * max_t + 4.0;
        let max_steps = if max_cells.is_finite() {
            max_cells.min(100_000.0) as u32 + 2
        } else {
            2
        };
        let mut t_now = 0.0_f32;
        for _ in 0..max_steps {
            if t_now > max_t {
                break;
            }
            if cx >= 0 && cz >= 0 && cx < w - 1 && cz < h - 1 {
                if let Some(hit) = self.ray_cell(cx, cz, origin, dir, max_t) {
                    return Some(hit);
                }
            } else if (cx < -1 && step_x < 0) || (cz < -1 && step_z < 0) || cx > w || cz > h {
                break; // left the world for good
            }
            // advance to the next cell
            if t_max_x < t_max_z {
                t_now = t_max_x;
                t_max_x += t_delta_x;
                cx += step_x;
            } else if t_max_z.is_finite() {
                t_now = t_max_z;
                t_max_z += t_delta_z;
                cz += step_z;
            } else {
                break; // a ray without horizontal motion only touches its start cell
            }
        }
        None
    }

    /// Both triangles of heightmap cell `(cx, cz)`, both windings; the nearest hit.
    fn ray_cell(&self, cx: i32, cz: i32, origin: Vec3, dir: Vec3, max_t: f32) -> Option<RayHit> {
        let m = self.mpu as f32;
        let centre_x = (cx as f32 + 0.5) * m;
        let centre_z = (cz as f32 + 0.5) * m;
        let land = self.land_index((cx / 128) as u32, (cz / 128) as u32)?;
        if !matches!(
            self.slot_by_index(land),
            Some(LandSlot::Loaded(_) | LandSlot::Missing)
        ) {
            return None;
        }
        let mut q = QueryCtx::default();
        let (a, b) = self.land_tri_pair_pub(centre_x, centre_z, &mut q)?;
        let mut best: Option<RayHit> = None;
        for tri in [a, b] {
            for flip in [false, true] {
                let (v1, v2) = if flip {
                    (tri[2], tri[1])
                } else {
                    (tri[1], tri[2])
                };
                if let Some(t) = ray_tri(tri[0], v1, v2, origin, dir)
                    && (0.0..=max_t).contains(&t)
                    && best.is_none_or(|bst| t < bst.t)
                {
                    let normal = normalize_or_zero(cross(tri[1] - tri[0], tri[2] - tri[0]));
                    best = Some(RayHit {
                        t,
                        pos: origin + dir * t,
                        kind: RAY_KIND_TERRAIN,
                        id: land as u32,
                        normal,
                    });
                }
            }
        }
        best
    }

    fn ray_statics(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<RayHit> {
        let end = origin + dir * max_t;
        let size = (128 * self.mpu) as f32;
        let (min_x, max_x) = (origin.x.min(end.x), origin.x.max(end.x));
        let (min_z, max_z) = (origin.z.min(end.z), origin.z.max(end.z));
        let clamp = |v: f32, hi: u32| ftoi_x86((v / size).floor()).clamp(0, hi as i32 - 1) as u32;
        let (lx0, lx1) = (clamp(min_x, self.lands_w), clamp(max_x, self.lands_w));
        let (lz0, lz1) = (clamp(min_z, self.lands_h), clamp(max_z, self.lands_h));
        let seg = Segment3::new(origin, end);
        let mut best: Option<RayHit> = None;
        for lz in lz0..=lz1 {
            for lx in lx0..=lx1 {
                let Some(idx) = self.land_index(lx, lz) else {
                    continue;
                };
                let Some(LandSlot::Loaded(chunk)) = self.slot_by_index(idx) else {
                    continue;
                };
                for (si, inst) in chunk.instances.iter().enumerate() {
                    if !seg_obb_test(&seg, &inst.obb) {
                        continue;
                    }
                    let Some(mesh) = chunk.meshes.get(inst.mesh as usize) else {
                        continue;
                    };
                    let ia = transform_coord(origin, &inst.inv_world);
                    let ib = transform_coord(origin + dir, &inst.inv_world);
                    let idir = ib - ia;
                    for ti in 0..mesh.tris.len() {
                        let Some(tri) = mesh.triangle(ti) else {
                            continue;
                        };
                        let Some(t) = ray_tri(tri[0], tri[1], tri[2], ia, idir) else {
                            continue;
                        };
                        if (0.0..=max_t).contains(&t) && best.is_none_or(|b| t < b.t) {
                            let n = cross(tri[1] - tri[0], tri[2] - tri[0]);
                            let normal = normalize_or_zero(transform_normal(n, &inst.world));
                            best = Some(RayHit {
                                t,
                                pos: origin + dir * t,
                                kind: RAY_KIND_STATIC,
                                id: (idx as u32).wrapping_mul(65536).wrapping_add(si as u32),
                                normal,
                            });
                        }
                    }
                }
            }
        }
        best
    }
}

fn ray_ctrls(dynamic: &DynamicColliders, origin: Vec3, dir: Vec3, max_t: f32) -> Option<RayHit> {
    let end = origin + dir * max_t;
    let seg = Segment3::new(origin, end);
    let mut best: Option<RayHit> = None;
    for c in dynamic.iter() {
        if !seg_obb_test(&seg, &c.obb) {
            continue;
        }
        for mr in dynamic.meshes(c) {
            let ia = transform_coord(origin, mr.inv_world);
            let ib = transform_coord(origin + dir, mr.inv_world);
            let idir = ib - ia;
            for ti in 0..mr.mesh.tris.len() {
                let Some(tri) = mr.mesh.triangle(ti) else {
                    continue;
                };
                let Some(t) = ray_tri(tri[0], tri[1], tri[2], ia, idir) else {
                    continue;
                };
                if (0.0..=max_t).contains(&t) && best.is_none_or(|b| t < b.t) {
                    let n = cross(tri[1] - tri[0], tri[2] - tri[0]);
                    best = Some(RayHit {
                        t,
                        pos: origin + dir * t,
                        kind: RAY_KIND_CTRL,
                        id: c.id.0,
                        normal: normalize_or_zero(transform_normal(n, mr.world)),
                    });
                }
            }
        }
    }
    best
}
