//! Geometry primitives (Plan/31 §10.5; `src:_Common/xUtil3D.cpp:120-410`; WP-SIM-03).
//!
//! Exact transcriptions (operation order included) of the original segment/box/triangle tests used by the
//! world queries. `D3DX` matrix helpers use the row-vector convention of [`D3dMatrix`].

use super::chunk::{INSTANCE_FLAG_ALWAYS_OBB, Obb, StaticInstance};
use crate::num::{Vec3, cross, dot, length, normalize_or_zero};
use flyff_common::math::D3dMatrix;

/// An axis-aligned bounding box.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Aabb {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

/// `Segment3`: centre, unit direction and half length.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Segment3 {
    /// `(a + b) * 0.5`.
    pub origin: Vec3,
    /// `normalize(b - a)` (zero for a zero-length segment).
    pub dir: Vec3,
    /// `|b - a| * 0.5`.
    pub extent: f32,
}

impl Segment3 {
    /// `Segment3(a, b)`.
    pub fn new(a: Vec3, b: Vec3) -> Segment3 {
        let d = b - a;
        let extent = length(d) * 0.5;
        Segment3 {
            origin: (a + b) * 0.5,
            dir: normalize_or_zero(d),
            extent,
        }
    }
}

#[inline]
fn m(mat: &D3dMatrix, r: usize, c: usize) -> f32 {
    mat.0.get(r * 4 + c).copied().unwrap_or(0.0)
}

/// `D3DXVec3TransformCoord` (row-vector `v * M`) with the affine fast path of Plan/31 §10.5: the divide
/// by `w` is skipped when `w == 1.0`.
pub fn transform_coord(v: Vec3, mat: &D3dMatrix) -> Vec3 {
    let x = ((v.x * m(mat, 0, 0) + v.y * m(mat, 1, 0)) + v.z * m(mat, 2, 0)) + m(mat, 3, 0);
    let y = ((v.x * m(mat, 0, 1) + v.y * m(mat, 1, 1)) + v.z * m(mat, 2, 1)) + m(mat, 3, 1);
    let z = ((v.x * m(mat, 0, 2) + v.y * m(mat, 1, 2)) + v.z * m(mat, 2, 2)) + m(mat, 3, 2);
    let w = ((v.x * m(mat, 0, 3) + v.y * m(mat, 1, 3)) + v.z * m(mat, 2, 3)) + m(mat, 3, 3);
    if w == 1.0 || w == 0.0 || !w.is_finite() {
        Vec3::new(x, y, z)
    } else {
        Vec3::new(x / w, y / w, z / w)
    }
}

/// `TransformCoord` with the matrix's translation row zeroed (`mInv._41 = mInv._42 = mInv._43 = 0`).
pub fn transform_coord_no_translation(v: Vec3, mat: &D3dMatrix) -> Vec3 {
    let mut z = *mat;
    for c in 0..3 {
        if let Some(slot) = z.0.get_mut(12 + c) {
            *slot = 0.0;
        }
    }
    transform_coord(v, &z)
}

/// `D3DXVec3TransformNormal`: the linear part only (`x*m00 + y*m10 + z*m20`, no translation, no divide).
pub fn transform_normal(v: Vec3, mat: &D3dMatrix) -> Vec3 {
    Vec3::new(
        (v.x * m(mat, 0, 0) + v.y * m(mat, 1, 0)) + v.z * m(mat, 2, 0),
        (v.x * m(mat, 0, 1) + v.y * m(mat, 1, 1)) + v.z * m(mat, 2, 1),
        (v.x * m(mat, 0, 2) + v.y * m(mat, 1, 2)) + v.z * m(mat, 2, 2),
    )
}

const CLIP_LEFT: u8 = 0x01;
const CLIP_RIGHT: u8 = 0x02;
const CLIP_BOTTOM: u8 = 0x04;
const CLIP_TOP: u8 = 0x08;
const CLIP_FRONT: u8 = 0x10;
const CLIP_BACK: u8 = 0x20;

fn out_code(min: Vec3, max: Vec3, v: Vec3) -> u8 {
    let mut c = 0;
    if v.x > max.x {
        c |= CLIP_RIGHT;
    } else if v.x < min.x {
        c |= CLIP_LEFT;
    }
    if v.y > max.y {
        c |= CLIP_TOP;
    } else if v.y < min.y {
        c |= CLIP_BOTTOM;
    }
    if v.z > max.z {
        c |= CLIP_BACK;
    } else if v.z < min.z {
        c |= CLIP_FRONT;
    }
    c
}

/// `IsTouchAABB_Line` (Cohen-Sutherland): the intersection point of the segment `a -> b` with the box,
/// `a` or `b` itself when one of them is inside.
pub fn aabb_line(min: Vec3, max: Vec3, a: Vec3, b: Vec3) -> Option<Vec3> {
    let oc1 = out_code(min, max, a);
    if oc1 == 0 {
        return Some(a);
    }
    let oc2 = out_code(min, max, b);
    if oc2 == 0 {
        return Some(b);
    }
    if oc1 & oc2 != 0 {
        return None;
    }
    let mut p = Vec3::ZERO;
    if oc1 & (CLIP_RIGHT | CLIP_LEFT) != 0 {
        p.x = if oc1 & CLIP_RIGHT != 0 { max.x } else { min.x };
        let x1 = b.x - a.x;
        let x2 = p.x - a.x;
        p.y = a.y + x2 * (b.y - a.y) / x1;
        p.z = a.z + x2 * (b.z - a.z) / x1;
        if p.y <= max.y && p.y >= min.y && p.z <= max.z && p.z >= min.z {
            return Some(p);
        }
    }
    if oc1 & (CLIP_TOP | CLIP_BOTTOM) != 0 {
        p.y = if oc1 & CLIP_TOP != 0 { max.y } else { min.y };
        let y1 = b.y - a.y;
        let y2 = p.y - a.y;
        p.x = a.x + y2 * (b.x - a.x) / y1;
        p.z = a.z + y2 * (b.z - a.z) / y1;
        if p.x <= max.x && p.x >= min.x && p.z <= max.z && p.z >= min.z {
            return Some(p);
        }
    }
    if oc1 & (CLIP_FRONT | CLIP_BACK) != 0 {
        p.z = if oc1 & CLIP_BACK != 0 { max.z } else { min.z };
        let z1 = b.z - a.z;
        let z2 = p.z - a.z;
        p.x = a.x + z2 * (b.x - a.x) / z1;
        p.y = a.y + z2 * (b.y - a.y) / z1;
        if p.x <= max.x && p.x >= min.x && p.y <= max.y && p.y >= min.y {
            return Some(p);
        }
    }
    None
}

/// `IntrSegment3Box3_Test` (V15 `__BS_BBOX_ABS_EXTENT`: `|extent|`): a separating-axis test of a segment
/// against an oriented box.
pub fn seg_obb_test(seg: &Segment3, obb: &Obb) -> bool {
    let diff = seg.origin - obb.center;
    let mut awdu = [0.0_f32; 3];
    for i in 0..3 {
        let axis = obb.axis.get(i).copied().unwrap_or(Vec3::ZERO);
        let ext = obb.extent.get(i).copied().unwrap_or(0.0);
        let w = dot(seg.dir, axis).abs();
        let d = dot(diff, axis).abs();
        if let Some(s) = awdu.get_mut(i) {
            *s = w;
        }
        let rhs = ext.abs() + seg.extent * w;
        if d > rhs {
            return false;
        }
    }
    let wxd = cross(seg.dir, diff);
    let ax = |i: usize| obb.axis.get(i).copied().unwrap_or(Vec3::ZERO);
    let ex = |i: usize| obb.extent.get(i).copied().unwrap_or(0.0).abs();
    let aw = |i: usize| awdu.get(i).copied().unwrap_or(0.0);
    let a0 = dot(wxd, ax(0)).abs();
    if a0 > ex(1) * aw(2) + ex(2) * aw(1) {
        return false;
    }
    let a1 = dot(wxd, ax(1)).abs();
    if a1 > ex(0) * aw(2) + ex(2) * aw(0) {
        return false;
    }
    let a2 = dot(wxd, ax(2)).abs();
    a2 <= ex(0) * aw(1) + ex(1) * aw(0)
}

/// `TestIntersectionOBB_Line` (`CModelObject`, server `__CPU_UTILDOWN_060502` branch): the instance
/// `ALWAYS_OBB` flag (`obj_guildcombatground.o3d`) always hits; otherwise the segment `a -> b` against the
/// instance OBB.
pub fn obb_line_test(inst: &StaticInstance, a: Vec3, b: Vec3) -> bool {
    if inst.flags & INSTANCE_FLAG_ALWAYS_OBB != 0 {
        return true;
    }
    seg_obb_test(&Segment3::new(a, b), &inst.obb)
}

/// `IsTouchOBB_Line` (`CModelObject::IsTouchOBB_Line`): the segment is moved into the instance's model
/// space with `inv_world`, intersected with the model AABB `[amin, amax]` and the hit mapped back by
/// `world`. No `ALWAYS_OBB` shortcut.
pub fn obb_line_point(
    inst: &StaticInstance,
    amin: Vec3,
    amax: Vec3,
    a: Vec3,
    b: Vec3,
) -> Option<Vec3> {
    let ia = transform_coord(a, &inst.inv_world);
    let ib = transform_coord(b, &inst.inv_world);
    aabb_line(amin, amax, ia, ib).map(|p| transform_coord(p, &inst.world))
}

/// `IsTouchRayTri` (Moeller-Trumbore, **back-face culled**): the ray parameter `t` in units of `dir`
/// (any sign). `None` for a back face, a miss or a non-finite result.
pub fn ray_tri(v0: Vec3, v1: Vec3, v2: Vec3, orig: Vec3, dir: Vec3) -> Option<f32> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let pvec = cross(dir, edge2);
    let det = dot(edge1, pvec);
    if det <= 0.0 || det.is_nan() {
        return None;
    }
    if det < 0.0001 {
        return None;
    }
    let tvec = orig - v0;
    let u = dot(tvec, pvec);
    if u < 0.0 || u > det {
        return None;
    }
    let qvec = cross(tvec, edge1);
    let v = dot(dir, qvec);
    if v < 0.0 || u + v > det {
        return None;
    }
    let mut t = dot(edge2, qvec);
    let inv_det = 1.0_f32 / det;
    t *= inv_det;
    if t.is_finite() { Some(t) } else { None }
}

/// `CalcSlideVec(d, n)`: `d - (-n) * (d . -n)` (removes the normal component; `n` is a unit vector).
pub fn calc_slide_vec(d: Vec3, n: Vec3) -> Vec3 {
    let inv_n = Vec3::new(-n.x, -n.y, -n.z);
    let b = dot(d, inv_n);
    d - inv_n * b
}

/// `CanSlide(v0, v1, v2, cosine)`: when the triangle's unit normal `n` satisfies `n . (0, -1, 0) > cosine`
/// (the surface is steeper than `acos(-cosine)`), the slide vector `calc_slide_vec((0, -0.2, 0), n)`.
pub fn can_slide(v0: Vec3, v1: Vec3, v2: Vec3, cosine: f32) -> Option<Vec3> {
    let u = v1 - v0;
    let v = v2 - v0;
    let n = normalize_or_zero(cross(u, v));
    let down = Vec3::new(0.0, -1.0, 0.0);
    if dot(n, down) > cosine {
        Some(calc_slide_vec(Vec3::new(0.0, -0.2, 0.0), n))
    } else {
        None
    }
}
