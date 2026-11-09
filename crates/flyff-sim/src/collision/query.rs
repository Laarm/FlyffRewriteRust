//! Mesh tests and world queries (Plan/31 §10.6–10.7; `src:_Common/Object3D.cpp:1695-2110`,
//! `WorldIntersect.cpp`; WP-SIM-03).
//!
//! The mesh tests transform the world segment into object space with the stored `inv_world`, visit the
//! triangles in index order and return on the **first** qualifying triangle. The world queries enumerate
//! candidates through the static link walk ([`CollisionWorld::for_linkmap`]) so "first hit wins" matches
//! the original order.

use super::chunk::{CollisionMesh, StaticInstance};
use super::dynamic::DynamicColliders;
use super::geom::{
    Segment3, calc_slide_vec, can_slide, obb_line_point, obb_line_test, ray_tri, seg_obb_test,
    transform_coord, transform_coord_no_translation, transform_normal,
};
use super::linkmap::StaticRef;
use super::terrain::QueryCtx;
use super::world::CollisionWorld;
use crate::num::{Vec3, cross, dot, length, length_sq, normalize_or_zero};
use core::ops::ControlFlow;
use flyff_common::math::D3dMatrix;

/// A collision mesh together with its instance transform (static instances and ctrl colliders).
#[derive(Clone, Copy)]
pub struct MeshRef<'a> {
    /// The object-space triangles.
    pub mesh: &'a CollisionMesh,
    /// Object -> world.
    pub world: &'a D3dMatrix,
    /// World -> object.
    pub inv_world: &'a D3dMatrix,
}

/// A tested segment in object space.
struct ObjSegment {
    ia: Vec3,
    idir: Vec3,
}

impl MeshRef<'_> {
    fn object_segment(&self, a: Vec3, b: Vec3) -> ObjSegment {
        let ia = transform_coord(a, self.inv_world);
        let ib = transform_coord(b, self.inv_world);
        ObjSegment { ia, idir: ib - ia }
    }

    /// The world-space normal of an object-space triangle: `cross(v2 - v1, v3 - v1)` transformed by the
    /// linear part of `world` (KEEP-QUIRK: not the inverse transpose), `y` zeroed, normalised.
    fn wall_normal(&self, t: &[Vec3; 3]) -> Vec3 {
        let n = cross(t[1] - t[0], t[2] - t[0]);
        let mut n = transform_coord_no_translation(n, self.world);
        n.y = 0.0;
        normalize_or_zero(n)
    }
}

/// `CObject3D::SlideVectorXZ2` (`__FIX_COLLISION`, Plan/31 §10.6): the slide vector of the segment `a -> b`
/// against the mesh and the hit point, `None` when no triangle qualifies.
///
/// Segments longer than 3 units use the long-probe branch (`|t| < |dir|`, a 0.2-scaled reflection),
/// shorter ones need `0 <= t < 1` and return the slide of the remaining part `b - hit`. This is the
/// faithful port; [`slide_vector_xz2_whole`] is what [`CollisionWorld::process_collision`] uses.
pub fn slide_vector_xz2(m: MeshRef<'_>, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    slide_xz2(m, a, b, false)
}

/// [`slide_vector_xz2`] with the **FIX** of the wall slide rate (`PL-002` follow-up): a short probe
/// slides with the whole tangential component of `b - a` instead of the tangential part of the
/// remainder `b - hit` behind the wall.
///
/// The original moved the mover by `slide / 6.5` where `slide` is the remainder past the first contact. A mover
/// that comes up to a wall stops approaching it (the normal part is removed), so the remainder stays at the
/// sliver by which the probe tip first poked through: at most `1 / 6.5` of the probe at one collision per
/// frame (the 4 frames of a 15 Hz tick gave the original server `4 / 6.5`, PL-002 took that away). On the
/// real Flaris walls a run 30 degrees off the wall crept at 0.14 m/s (2 % of 6 m/s) and one 45 degrees off at
/// 0.85 m/s (14 %), a wall-crawl nobody could use; the same mover now keeps `sin(theta)` of its speed. Head-on
/// probes (`theta = 0`) still stop, the second hit of the one allowed recursion still zeroes a corner, and no
/// normal component is ever added, so the mover never gets closer to the wall than the look-ahead distance
/// of its first contact.
pub fn slide_vector_xz2_whole(m: MeshRef<'_>, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    slide_xz2(m, a, b, true)
}

fn slide_xz2(m: MeshRef<'_>, a: Vec3, b: Vec3, whole: bool) -> Option<(Vec3, Vec3)> {
    let seg = m.object_segment(a, b);
    let dir = b - a;
    let len = length(dir);
    for ti in 0..m.mesh.tris.len() {
        let Some(tri) = m.mesh.triangle(ti) else {
            continue;
        };
        let Some(t) = ray_tri(tri[0], tri[1], tri[2], seg.ia, seg.idir) else {
            continue;
        };
        if len > 3.0 {
            if t.abs() < len {
                let hit = if t < 0.0 { a - dir * t } else { a + dir * t };
                let n = m.wall_normal(&tri);
                let inv_dir = Vec3::new(-dir.x, -dir.y, -dir.z);
                let f = dot(inv_dir, n);
                let r = dir + n * (f * 2.0);
                return Some((r * 0.2, hit));
            }
        } else if (0.0..1.0).contains(&t) {
            let hit = a + dir * t;
            let n = m.wall_normal(&tri);
            let rest = if whole { dir } else { b - hit };
            return Some((calc_slide_vec(rest, n), hit));
        }
    }
    None
}

/// `CObject3D::SlideVectorUnder` (`__SLIDE_060502`, the server variant on both sides, PL-005): for the
/// first triangle with `0 <= t < 1` the point `hit = a + t * (b - a)` and either `(0, 0, 0)` (surface not
/// steeper than 101 degrees) or `hit + TransformNormal(slide, world)` with the object-space down vector
/// (KEEP-QUIRK).
pub fn slide_vector_under(m: MeshRef<'_>, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    let seg = m.object_segment(a, b);
    for ti in 0..m.mesh.tris.len() {
        let Some(tri) = m.mesh.triangle(ti) else {
            continue;
        };
        let Some(t) = ray_tri(tri[0], tri[1], tri[2], seg.ia, seg.idir) else {
            continue;
        };
        if (0.0..1.0).contains(&t) {
            let hit = a + (b - a) * t;
            return Some(match can_slide(tri[0], tri[1], tri[2], -0.190_80) {
                None => (Vec3::ZERO, hit),
                Some(s) => (hit + transform_normal(s, m.world), hit),
            });
        }
    }
    None
}

/// `CObject3D::FindTouchTriLine`: the first triangle with `0 <= t < 1` (object-space vertices) and `t`.
pub fn find_touch_tri_line(m: MeshRef<'_>, a: Vec3, b: Vec3) -> Option<([Vec3; 3], f32)> {
    let seg = m.object_segment(a, b);
    for ti in 0..m.mesh.tris.len() {
        let Some(tri) = m.mesh.triangle(ti) else {
            continue;
        };
        if let Some(t) = ray_tri(tri[0], tri[1], tri[2], seg.ia, seg.idir)
            && (0.0..1.0).contains(&t)
        {
            return Some((tri, t));
        }
    }
    None
}

fn static_mesh<'a>(r: &StaticRef<'a>) -> Option<(&'a StaticInstance, MeshRef<'a>)> {
    let inst = r.instance()?;
    let mesh = r.mesh()?;
    Some((
        inst,
        MeshRef {
            mesh,
            world: &inst.world,
            inv_world: &inst.inv_world,
        },
    ))
}

impl CollisionWorld {
    /// `GetFullHeight(p)`: the height under `p` including static objects (movement uses it for the
    /// flight collision): over the statics of the link walk the OBB-line hit closest to `p` wins and its
    /// exact triangle gives `p.y + t * -2000`; otherwise the land height. `Pending` lands read as flat at
    /// `q.pending_y`.
    pub fn full_height_q(&self, p: Vec3, q: &mut QueryCtx) -> f32 {
        let end = p + Vec3::new(0.0, -2000.0, 0.0);
        let mut best_d = 4_194_304.0_f32;
        let mut best: Option<StaticRef<'_>> = None;
        self.for_linkmap(p, 0, |r| {
            if let Some((inst, mr)) = static_mesh(&r)
                && let Some(hit) =
                    obb_line_point(inst, aabb_min(mr.mesh), aabb_max(mr.mesh), p, end)
            {
                let d = length_sq(hit - p);
                if d < best_d {
                    best_d = d;
                    best = Some(r);
                }
            }
            ControlFlow::Continue(())
        });
        if let Some(r) = best
            && let Some((_, mr)) = static_mesh(&r)
            && let Some((_, t)) = find_touch_tri_line(mr, p, end)
        {
            return p.y + t * -2000.0;
        }
        self.land_height_q(p.x, p.z, q)
    }

    /// [`CollisionWorld::full_height_q`] without a mover context.
    pub fn full_height(&self, p: Vec3) -> f32 {
        self.full_height_q(p, &mut QueryCtx::default())
    }

    /// `GetUnderHeight(p)` (spawn placement): the FIRST static whose OBB the segment `p -> p - 1000 y`
    /// hits, its first triangle `p.y + t * -1000`; otherwise the land height.
    pub fn under_height_q(&self, p: Vec3, q: &mut QueryCtx) -> f32 {
        let end = p + Vec3::new(0.0, -1000.0, 0.0);
        let mut first: Option<StaticRef<'_>> = None;
        self.for_linkmap(p, 0, |r| {
            if let Some((inst, _)) = static_mesh(&r)
                && obb_line_test(inst, p, end)
            {
                first = Some(r);
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        });
        if let Some(r) = first
            && let Some((_, mr)) = static_mesh(&r)
            && let Some((_, t)) = find_touch_tri_line(mr, p, end)
        {
            return p.y + t * -1000.0;
        }
        self.land_height_q(p.x, p.z, q)
    }

    /// [`CollisionWorld::under_height_q`] without a mover context.
    pub fn under_height(&self, p: Vec3) -> f32 {
        self.under_height_q(p, &mut QueryCtx::default())
    }

    /// `GetItemHeight(p)` (dropped items): the highest `slide_vector_under` hit within 3 units below `p`
    /// over all candidate statics; otherwise the land height.
    pub fn item_height_q(&self, p: Vec3, q: &mut QueryCtx) -> f32 {
        let end = p + Vec3::new(0.0, -3.0, 0.0);
        let mut max_y = -65535.0_f32;
        self.for_linkmap(p, 0, |r| {
            if let Some((inst, mr)) = static_mesh(&r)
                && obb_line_test(inst, p, end)
                && let Some((_, hit)) = slide_vector_under(mr, p, end)
                && hit.y > max_y
            {
                max_y = hit.y;
            }
            ControlFlow::Continue(())
        });
        if max_y != -65535.0 {
            return max_y;
        }
        self.land_height_q(p.x, p.z, q)
    }

    /// [`CollisionWorld::item_height_q`] without a mover context.
    pub fn item_height(&self, p: Vec3) -> f32 {
        self.item_height_q(p, &mut QueryCtx::default())
    }

    /// `GetOverHeight(p)` (ceiling): the FIRST static whose OBB the segment `p -> p + 1 y` hits, its first
    /// triangle `p.y + t`; `65535.0` when nothing is above.
    pub fn over_height(&self, p: Vec3) -> f32 {
        let end = p + Vec3::new(0.0, 1.0, 0.0);
        let mut first: Option<StaticRef<'_>> = None;
        self.for_linkmap(p, 0, |r| {
            if let Some((inst, _)) = static_mesh(&r)
                && obb_line_test(inst, p, end)
            {
                first = Some(r);
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        });
        self.over_height_of(first, p, end)
    }

    fn over_height_of(&self, hit: Option<StaticRef<'_>>, p: Vec3, end: Vec3) -> f32 {
        if let Some(r) = hit
            && let Some((_, mr)) = static_mesh(&r)
            && let Some((_, t)) = find_touch_tri_line(mr, p, end)
        {
            return p.y + t * 1.0;
        }
        65535.0
    }

    /// `GetOverHeightForPlayer(p)` (`__BOUND_BOX_COLLISION`): like [`CollisionWorld::over_height`] but a
    /// static whose OBB contains `p` itself (the zero-length segment `(p, p)` also hits) is remembered and
    /// the walk continues; the first object that is hit by the segment but does not contain `p` ends it.
    pub fn over_height_for_player(&self, p: Vec3) -> f32 {
        let end = p + Vec3::new(0.0, 1.0, 0.0);
        let mut chosen: Option<StaticRef<'_>> = None;
        let point = Segment3::new(p, p);
        self.for_linkmap(p, 0, |r| {
            if let Some((inst, _)) = static_mesh(&r)
                && obb_line_test(inst, p, end)
            {
                chosen = Some(r);
                let contains = inst.flags & super::chunk::INSTANCE_FLAG_ALWAYS_OBB != 0
                    || seg_obb_test(&point, &inst.obb);
                if !contains {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        });
        self.over_height_of(chosen, p, end)
    }

    /// `CWorld::ProcessCollision(a, dir, depth)`: the horizontal collision probe. Returns `(kind, out)`:
    /// `1` when a dynamic ctrl was hit, `2` for a static, `0` for no hit (`out = dir`). `out` is the slide
    /// vector ([`slide_vector_xz2_whole`]: the whole tangential component of a short probe, FIX), recursed once
    /// from `a` (KEEP-QUIRK: not from the hit point); two consecutive hits give a zero vector (a corner).
    pub fn process_collision(
        &self,
        dynamic: &DynamicColliders,
        a: Vec3,
        dir: Vec3,
        depth: i32,
    ) -> (u8, Vec3) {
        let end = a + dir;
        let seg = Segment3::new(a, end);
        // (1) dynamic ctrls: OBB test of the render bounds (no collision-mesh requirement), then the posed
        // render meshes of the ctrl object by object, first triangle wins (`SlideVectorXZ2`, `bCollObj = 0`)
        let mut dyn_hit: Option<Vec3> = None;
        dynamic.walk(a, |c| {
            if seg_obb_test(&seg, &c.obb) {
                for mr in dynamic.meshes(c) {
                    if let Some((slide, _hit)) = slide_vector_xz2_whole(mr, a, end) {
                        dyn_hit = Some(slide);
                        return ControlFlow::Break(());
                    }
                }
            }
            ControlFlow::Continue(())
        });
        if let Some(slide) = dyn_hit {
            let mut out = slide;
            if depth < 1 {
                let (k, o) = self.process_collision(dynamic, a, slide, depth + 1);
                out = if k != 0 { Vec3::ZERO } else { o };
            }
            return (1, out);
        }
        // (2) statics in link order
        let mut result: Option<Vec3> = None;
        self.for_linkmap(a, 0, |r| {
            if let Some((inst, mr)) = static_mesh(&r)
                && obb_line_test(inst, a, end)
                && let Some((slide, _hit)) = slide_vector_xz2_whole(mr, a, end)
            {
                result = Some(slide);
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        });
        if let Some(slide) = result {
            let mut out = slide;
            if depth < 1 {
                let (k, o) = self.process_collision(dynamic, a, slide, depth + 1);
                out = if k != 0 { Vec3::ZERO } else { o };
            }
            return (2, out);
        }
        (0, dir)
    }

    /// `CWorld::ProcessCollisionReflection(a, dir, depth)`: the flight reflection off the first static hit
    /// (`out = dir + n * 2(-dir . n)`), recursing up to three more times from `a`.
    pub fn process_collision_reflection(&self, a: Vec3, dir: Vec3, depth: i32) -> Option<Vec3> {
        let end = a + dir;
        let mut found: Option<Vec3> = None;
        self.for_linkmap(a, 0, |r| {
            if let Some((inst, mr)) = static_mesh(&r)
                && obb_line_test(inst, a, end)
                && let Some((tri, _)) = find_touch_tri_line(mr, a, end)
            {
                let n = cross(tri[1] - tri[0], tri[2] - tri[0]);
                let n = normalize_or_zero(transform_coord_no_translation(n, mr.world));
                let inv_dir = Vec3::new(-dir.x, -dir.y, -dir.z);
                let len_n = dot(inv_dir, n);
                found = Some(dir + n * (len_n * 2.0));
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(())
        });
        let mut out = found?;
        if depth < 3
            && let Some(o) = self.process_collision_reflection(a, out, depth + 1)
        {
            out = o;
        }
        Some(out)
    }

    /// `CWorld::ProcessUnderCollision(p)` (`__SLIDE_060502`, PL-005): probes `p -> p + (0, -1.5, 0)`
    /// against the statics (and, in housing worlds, the dynamic ctrls first) keeping the highest hit, then
    /// the terrain triangle. Returns `(ground height, slide position)`; `slide.y != 0` means "slide
    /// here". **FIX**: when the terrain ray misses (rounding only) the plane of the triangle is used (the
    /// original read an uninitialised value).
    pub fn process_under_collision(
        &self,
        dynamic: &DynamicColliders,
        p: Vec3,
        q: &mut QueryCtx,
    ) -> (f32, Vec3) {
        let dir = Vec3::new(0.0, -1.5, 0.0);
        let end = p + dir;
        let mut max_y = -65535.0_f32;
        let mut slide = Vec3::ZERO;
        if self.housing {
            let seg = Segment3::new(p, end);
            dynamic.walk(p, |c| {
                if seg_obb_test(&seg, &c.obb) {
                    for mr in dynamic.meshes(c) {
                        if let Some((out, hit)) = slide_vector_under(mr, p, end)
                            && hit.y > max_y
                        {
                            max_y = hit.y;
                            slide = out;
                        }
                    }
                }
                ControlFlow::Continue(())
            });
        }
        self.for_linkmap(p, 0, |r| {
            if let Some((inst, mr)) = static_mesh(&r)
                && obb_line_test(inst, p, end)
                && let Some((out, hit)) = slide_vector_under(mr, p, end)
                && hit.y > max_y
            {
                max_y = hit.y;
                slide = out;
            }
            ControlFlow::Continue(())
        });
        // terrain: the triangle under p and the ray hit
        let tri = self.land_tri_q(p.x, p.z, q).unwrap_or([Vec3::ZERO; 3]);
        let t = match ray_tri(tri[0], tri[1], tri[2], p, dir) {
            Some(t) => t,
            None => plane_t(&tri, p, dir),
        };
        let ly = p.y + t * dir.y;
        if max_y > ly {
            // an object is above the land: it cannot slide below the land
            if slide.y != 0.0 && slide.y < ly {
                slide.y = ly;
            }
        } else {
            slide = Vec3::ZERO;
            max_y = ly;
            if let Some(s) = can_slide(tri[0], tri[1], tri[2], -0.378_64) {
                let hit = Vec3::new(p.x, ly, p.z);
                let tris = self.land_tris_3x3_q(p.x, p.z, q);
                let mut found = None;
                for t3 in tris.iter() {
                    if let Some(t2) = ray_tri(t3[0], t3[1], t3[2], hit, s)
                        && t2 >= 0.0
                    {
                        found = Some(t2);
                        break;
                    }
                }
                slide = match found {
                    Some(t2) => hit + s * t2,
                    None => hit + s,
                };
            }
        }
        (max_y, slide)
    }
}

/// Ray parameter of the plane of `tri` for a vertical ray (the FIX of `process_under_collision` when
/// `ray_tri` rejects the triangle): `0.0` for a degenerate or vertical plane.
fn plane_t(tri: &[Vec3; 3], orig: Vec3, dir: Vec3) -> f32 {
    let n = normalize_or_zero(cross(tri[1] - tri[0], tri[2] - tri[0]));
    let denom = dot(n, dir);
    if denom == 0.0 || denom.is_nan() {
        return 0.0;
    }
    let t = dot(n, tri[0] - orig) / denom;
    if t.is_finite() { t } else { 0.0 }
}

#[inline]
fn aabb_min(m: &CollisionMesh) -> Vec3 {
    Vec3::new(m.aabb_min[0], m.aabb_min[1], m.aabb_min[2])
}

#[inline]
fn aabb_max(m: &CollisionMesh) -> Vec3 {
    Vec3::new(m.aabb_max[0], m.aabb_max[1], m.aabb_max[2])
}
