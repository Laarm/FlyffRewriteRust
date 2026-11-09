//! Quaternion and matrix primitives of the pose evaluator (Plan/31 §13.3; WP-SIM-12).
//!
//! Canonical D3DX formulas in the row-vector convention (`v' = v · M`, `World = Local · Parent`),
//! transcribed so that the operation order is fixed (left-to-right sums, no FMA, `libm` for the
//! transcendentals). Bit identity with D3DX itself is not required (Plan/31 §13.3): the oracle shims D3DX
//! with these formulas.
//!
//! Every function is total: finite input never produces NaN (the guards are marked `FIX`), and nothing
//! panics.

use crate::num::{Vec3, acosf, cosf, sinf, sqrtf, to_radian};
use flyff_common::math::D3dMatrix;

/// A 4x4 row-vector matrix (`D3DXMATRIX`), row-major (`_41.._43` = translation).
pub type Mat44 = D3dMatrix;

/// An affine matrix without its last column: 4 rows x 3 columns = 12 `f32`, row-major (Plan/31 §13.1).
/// The dropped fourth column is `(0, 0, 0, 1)`; the corpus stores `1 ± 2e-7` for `_44` in 10,782 of its
/// 25,669 `.chr` matrices, which the skinning shader of the original ignored as well (ref/18 §7.2).
pub type Mat43 = [f32; 12];

/// One animation key: rotation and translation (`TM_ANIMATION`, no scale).
pub type Key = (Quat, Vec3);

/// The identity [`Mat43`].
pub const MAT43_IDENTITY: Mat43 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

/// `D3DXQUATERNION`: `x, y, z, w`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quat {
    /// Vector part x.
    pub x: f32,
    /// Vector part y.
    pub y: f32,
    /// Vector part z.
    pub z: f32,
    /// Scalar part.
    pub w: f32,
}

impl Quat {
    /// The identity rotation.
    pub const IDENTITY: Quat = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// A quaternion from its four components.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Quat {
        Quat { x, y, z, w }
    }

    /// From `[x, y, z, w]` (the file order).
    pub const fn from_array(a: [f32; 4]) -> Quat {
        Quat::new(a[0], a[1], a[2], a[3])
    }

    /// To `[x, y, z, w]`.
    pub const fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }

    /// `D3DXQuaternionDot`: `ax*bx + ay*by + az*bz + aw*bw`, summed left to right.
    pub fn dot(self, o: Quat) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    /// `true` when all four components are finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }

    fn neg(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, -self.w)
    }
}

/// `D3DXQuaternionSlerp` (canonical form of Plan/31 §13.3): shortest arc, a plain lerp of the components
/// when the quaternions are (nearly) parallel (`1 - dot <= 1e-6`). The result is not renormalised.
///
/// `acosf` and `sinf` are `libm`; the argument of `acosf` is `<= 1 - 1e-6`, so `sinf(theta) > 0` and the
/// division is safe for finite input.
pub fn slerp(a: Quat, b: Quat, t: f32) -> Quat {
    let mut c = a.dot(b);
    let mut b = b;
    if c < 0.0 {
        b = b.neg();
        c = -c;
    }
    let (s0, s1) = if 1.0 - c > 1e-6 {
        let th = acosf(c.min(1.0));
        let sn = sinf(th);
        (sinf((1.0 - t) * th) / sn, sinf(t * th) / sn)
    } else {
        (1.0 - t, t)
    };
    Quat::new(
        a.x * s0 + b.x * s1,
        a.y * s0 + b.y * s1,
        a.z * s0 + b.z * s1,
        a.w * s0 + b.w * s1,
    )
}

/// `D3DXVec3Lerp`: `a + (b - a) * t`, component-wise.
pub fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    Vec3::new(
        a.x + (b.x - a.x) * t,
        a.y + (b.y - a.y) * t,
        a.z + (b.z - a.z) * t,
    )
}

/// `D3DXMatrixRotationQuaternion` (row-vector convention): `R(q)` with the translation row zero.
pub fn mat_from_quat(q: Quat) -> Mat44 {
    let Quat { x, y, z, w } = q;
    D3dMatrix([
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        0.0,
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        0.0,
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ])
}

/// `R(q) · T(p)` (`D3DXMatrixRotationQuaternion` then `*= D3DXMatrixTranslation`): the rotation rows of
/// `R(q)` and the translation `p` in the last row. This is the exact product (rotation rows have a zero
/// fourth element), written out instead of multiplied.
pub fn mat_rt(q: Quat, p: Vec3) -> Mat44 {
    let mut m = mat_from_quat(q);
    m.0[12] = p.x;
    m.0[13] = p.y;
    m.0[14] = p.z;
    m
}

/// `D3DXQuaternionRotationMatrix` for the rotation part (first three rows and columns) of an affine
/// matrix (Shoemake: the positive-trace branch, else the branch of the largest diagonal element, tested in
/// the order `m11 >= m22, m33` then `m22 >= m33`). A matrix with scale gives a non-unit, inexact
/// quaternion exactly like the original (the blend drops the scale, Plan/31 §13.2).
///
/// FIX: a non-finite input, or a degenerate branch (`s <= 0`), returns the identity instead of NaN.
pub fn quat_from_mat43(m: &Mat43) -> Quat {
    let [m11, m12, m13, m21, m22, m23, m31, m32, m33, _, _, _] = *m;
    let trace = m11 + m22 + m33;
    let q = if trace > 0.0 {
        let s = 0.5 / sqrtf(trace + 1.0);
        Quat::new((m23 - m32) * s, (m31 - m13) * s, (m12 - m21) * s, 0.25 / s)
    } else if m11 >= m22 && m11 >= m33 {
        let s = 2.0 * sqrtf(1.0 + m11 - m22 - m33);
        Quat::new(0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m23 - m32) / s)
    } else if m22 >= m33 {
        let s = 2.0 * sqrtf(1.0 + m22 - m11 - m33);
        Quat::new((m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m31 - m13) / s)
    } else {
        let s = 2.0 * sqrtf(1.0 + m33 - m11 - m22);
        Quat::new((m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m12 - m21) / s)
    };
    if q.is_finite() { q } else { Quat::IDENTITY }
}

/// `decompose(M)` of Plan/31 §13.2: the quaternion of the rotation part and the translation `_41.._43`.
pub fn decompose(m: &Mat43) -> Key {
    let [_, _, _, _, _, _, _, _, _, tx, ty, tz] = *m;
    (quat_from_mat43(m), Vec3::new(tx, ty, tz))
}

/// The 12 stored floats of an affine [`Mat44`] (the fourth column is dropped).
pub fn mat43_from_44(m: &Mat44) -> Mat43 {
    let [a, b, c, _, d, e, f, _, g, h, i, _, j, k, l, _] = m.0;
    [a, b, c, d, e, f, g, h, i, j, k, l]
}

/// The affine [`Mat44`] of a [`Mat43`]: the fourth column is exactly `(0, 0, 0, 1)`.
pub fn mat44_from_43(m: &Mat43) -> Mat44 {
    let [a, b, c, d, e, f, g, h, i, j, k, l] = *m;
    D3dMatrix([a, b, c, 0.0, d, e, f, 0.0, g, h, i, 0.0, j, k, l, 1.0])
}

/// `D3DXMatrixMultiply(a, b)`: `a · b` with the sums evaluated left to right
/// (`a_i1*b_1j + a_i2*b_2j + a_i3*b_3j + a_i4*b_4j`).
pub fn mul44(a: &Mat44, b: &Mat44) -> Mat44 {
    let [
        a11,
        a12,
        a13,
        a14,
        a21,
        a22,
        a23,
        a24,
        a31,
        a32,
        a33,
        a34,
        a41,
        a42,
        a43,
        a44,
    ] = a.0;
    let [
        b11,
        b12,
        b13,
        b14,
        b21,
        b22,
        b23,
        b24,
        b31,
        b32,
        b33,
        b34,
        b41,
        b42,
        b43,
        b44,
    ] = b.0;
    D3dMatrix([
        a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41,
        a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42,
        a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43,
        a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44,
        a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41,
        a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42,
        a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43,
        a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44,
        a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41,
        a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42,
        a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43,
        a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44,
        a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41,
        a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42,
        a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43,
        a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44,
    ])
}

/// `a · b` for affine matrices ([`Mat43`] storage): the product [`mul44`] gives when both fourth columns
/// are `(0, 0, 0, 1)`, without the multiplications by those constants (the sums are evaluated in the same
/// order, so every element equals the [`mul44`] element; only the sign of an exact zero can differ).
pub fn mul43(a: &Mat43, b: &Mat43) -> Mat43 {
    let [a11, a12, a13, a21, a22, a23, a31, a32, a33, a41, a42, a43] = *a;
    let [b11, b12, b13, b21, b22, b23, b31, b32, b33, b41, b42, b43] = *b;
    [
        a11 * b11 + a12 * b21 + a13 * b31,
        a11 * b12 + a12 * b22 + a13 * b32,
        a11 * b13 + a12 * b23 + a13 * b33,
        a21 * b11 + a22 * b21 + a23 * b31,
        a21 * b12 + a22 * b22 + a23 * b32,
        a21 * b13 + a22 * b23 + a23 * b33,
        a31 * b11 + a32 * b21 + a33 * b31,
        a31 * b12 + a32 * b22 + a33 * b32,
        a31 * b13 + a32 * b23 + a33 * b33,
        a41 * b11 + a42 * b21 + a43 * b31 + b41,
        a41 * b12 + a42 * b22 + a43 * b32 + b42,
        a41 * b13 + a42 * b23 + a43 * b33 + b43,
    ]
}

/// [`mul43`] over [`Mat44`]s (the fourth columns are assumed `(0, 0, 0, 1)` and the result has exactly that
/// column). This is the hot-path product of the pose evaluator.
pub fn mul_affine(a: &Mat44, b: &Mat44) -> Mat44 {
    mat44_from_43(&mul43(&mat43_from_44(a), &mat43_from_44(b)))
}

/// `D3DXMatrixRotationX(rad)` (left-handed, row vectors).
pub fn rotation_x(rad: f32) -> Mat44 {
    let (s, c) = (sinf(rad), cosf(rad));
    D3dMatrix([
        1.0, 0.0, 0.0, 0.0, //
        0.0, c, s, 0.0, //
        0.0, -s, c, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ])
}

/// `D3DXMatrixRotationZ(rad)` (left-handed, row vectors).
pub fn rotation_z(rad: f32) -> Mat44 {
    let (s, c) = (sinf(rad), cosf(rad));
    D3dMatrix([
        c, s, 0.0, 0.0, //
        -s, c, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ])
}

/// `D3DXToRadian` applied to a rotation about X in degrees.
pub fn rotation_x_deg(deg: f32) -> Mat44 {
    rotation_x(to_radian(deg))
}

/// `D3DXToRadian` applied to a rotation about Z in degrees.
pub fn rotation_z_deg(deg: f32) -> Mat44 {
    rotation_z(to_radian(deg))
}

/// `D3DXVec3TransformCoord` for an affine matrix (`w` is exactly 1, so no divide):
/// `((x*m11 + y*m21) + z*m31) + m41` per component.
pub fn transform_coord(m: &Mat44, v: Vec3) -> Vec3 {
    let [
        m11,
        m12,
        m13,
        _,
        m21,
        m22,
        m23,
        _,
        m31,
        m32,
        m33,
        _,
        m41,
        m42,
        m43,
        _,
    ] = m.0;
    Vec3::new(
        v.x * m11 + v.y * m21 + v.z * m31 + m41,
        v.x * m12 + v.y * m22 + v.z * m32 + m42,
        v.x * m13 + v.y * m23 + v.z * m33 + m43,
    )
}

/// The general 4x4 inverse (Plan/31 §13.4, used by §10.10): cofactor expansion from the twelve 2x2
/// sub-determinants of the top and bottom row pairs, in `f32`, in a fixed evaluation order. A singular
/// matrix (`|det| < 1e-12`, or a non-finite determinant) returns the identity.
pub fn inverse4x4(m: &Mat44) -> Mat44 {
    let [
        m00,
        m01,
        m02,
        m03,
        m10,
        m11,
        m12,
        m13,
        m20,
        m21,
        m22,
        m23,
        m30,
        m31,
        m32,
        m33,
    ] = m.0;
    let s0 = m00 * m11 - m10 * m01;
    let s1 = m00 * m12 - m10 * m02;
    let s2 = m00 * m13 - m10 * m03;
    let s3 = m01 * m12 - m11 * m02;
    let s4 = m01 * m13 - m11 * m03;
    let s5 = m02 * m13 - m12 * m03;
    let c5 = m22 * m33 - m32 * m23;
    let c4 = m21 * m33 - m31 * m23;
    let c3 = m21 * m32 - m31 * m22;
    let c2 = m20 * m33 - m30 * m23;
    let c1 = m20 * m32 - m30 * m22;
    let c0 = m20 * m31 - m30 * m21;
    let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
    if !det.is_finite() || det.abs() < 1e-12 {
        return D3dMatrix::IDENTITY;
    }
    let inv = 1.0 / det;
    D3dMatrix([
        (m11 * c5 - m12 * c4 + m13 * c3) * inv,
        (-m01 * c5 + m02 * c4 - m03 * c3) * inv,
        (m31 * s5 - m32 * s4 + m33 * s3) * inv,
        (-m21 * s5 + m22 * s4 - m23 * s3) * inv,
        (-m10 * c5 + m12 * c2 - m13 * c1) * inv,
        (m00 * c5 - m02 * c2 + m03 * c1) * inv,
        (-m30 * s5 + m32 * s2 - m33 * s1) * inv,
        (m20 * s5 - m22 * s2 + m23 * s1) * inv,
        (m10 * c4 - m11 * c2 + m13 * c0) * inv,
        (-m00 * c4 + m01 * c2 - m03 * c0) * inv,
        (m30 * s4 - m31 * s2 + m33 * s0) * inv,
        (-m20 * s4 + m21 * s2 - m23 * s0) * inv,
        (-m10 * c3 + m11 * c1 - m12 * c0) * inv,
        (m00 * c3 - m01 * c1 + m02 * c0) * inv,
        (-m30 * s3 + m31 * s1 - m32 * s0) * inv,
        (m20 * s3 - m21 * s1 + m22 * s0) * inv,
    ])
}

/// `true` when all twelve floats are finite.
pub fn mat43_is_finite(m: &Mat43) -> bool {
    m.iter().all(|v| v.is_finite())
}
