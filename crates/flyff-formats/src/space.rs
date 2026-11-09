//! Coordinate helpers: the left-handed Direct3D to three.js mapping (Plan/30 §12; ref/18 §14.2; Plan/01 §8).
//!
//! Server, protocol, sim and every model/pack in this crate are left-handed (+Y up, +Z north, row
//! vectors). The browser renderer uses three.js space; the mapping is `S = diag(1, 1, -1, 1)`:
//!
//! | data | mapping |
//! |---|---|
//! | positions, normals, event/force points | `(x, y, z) -> (x, y, -z)` ([`mirror_pos`]) |
//! | quaternions | `(x, y, z, w) -> (-x, -y, z, w)` ([`mirror_quat`]) |
//! | matrices | copy the 16 floats, negate indices 2, 6, 8, 9, 11, 14 = `S * M * S` ([`mirror_mat`]) |
//! | AABB | `z: (-max.z, -min.z)` ([`mirror_aabb`]) |
//! | triangle winding | `(a, b, c) -> (a, c, b)` ([`flip_winding_u16`]) |
//! | yaw | `rotation.y = +angle * pi / 180` ([`yaw_to_three`]) |
//!
//! Consistency proof for skinning: with mapped vertices `v' = v * S` and mapped palette `M' = S * M * S`,
//! `v' * M' = (v * M) * S`. All functions are pure and exact (negations and copies only, except
//! [`yaw_to_three`] and [`det3`]); a shared golden vector file `tests/golden/space.json` is checked by both
//! Rust and Vitest (`web/src/render/space.ts`).

use crate::raw::{D3dMatrix, Quat, Vec3};

/// `(x, y, z) -> (x, y, -z)`: positions, normals, event points, force points, paths.
pub fn mirror_pos(p: Vec3) -> Vec3 {
    [p[0], p[1], -p[2]]
}

/// `(x, y, z, w) -> (-x, -y, z, w)`.
pub fn mirror_quat(q: Quat) -> Quat {
    [-q[0], -q[1], q[2], q[3]]
}

/// `S * M * S` for `S = diag(1, 1, -1, 1)`: the 16 floats with indices 2, 6, 8, 9, 11 and 14 negated
/// (the `_13 _23 _31 _32 _34 _43` elements).
pub fn mirror_mat(m: &D3dMatrix) -> D3dMatrix {
    let mut out = m.0;
    for i in [2usize, 6, 8, 9, 11, 14] {
        if let Some(v) = out.get_mut(i) {
            *v = -*v;
        }
    }
    D3dMatrix(out)
}

/// The mirrored axis-aligned box: `z` becomes `(-max.z, -min.z)` so that `min <= max` still holds.
pub fn mirror_aabb(min: Vec3, max: Vec3) -> (Vec3, Vec3) {
    ([min[0], min[1], -max[2]], [max[0], max[1], -min[2]])
}

/// Reverses the winding of every complete triangle `(a, b, c) -> (a, c, b)` in place (a trailing
/// partial triangle is left unchanged).
pub fn flip_winding_u16(ib: &mut [u16]) {
    for tri in ib.as_chunks_mut::<3>().0 {
        tri.swap(1, 2);
    }
}

/// [`flip_winding_u16`] for 32-bit indices.
pub fn flip_winding_u32(ib: &mut [u32]) {
    for tri in ib.as_chunks_mut::<3>().0 {
        tri.swap(1, 2);
    }
}

/// The three.js `rotation.y` for a Flyff yaw `angle` in degrees: `+angle * pi / 180` (derivation: the
/// world rotation is `RotY(-angle)` in D3D row-vector form and `S * RotY(-a) * S = RotY(+a)`). Computed in
/// double precision and rounded once, like the TypeScript mirror.
pub fn yaw_to_three(angle_deg: f32) -> f32 {
    (f64::from(angle_deg) * core::f64::consts::PI / 180.0) as f32
}

/// Determinant of the upper-left 3x3 block (mirrored-node detection: a negative value means the node
/// transform flips handedness; 29 corpus objects have one).
pub fn det3(m: &D3dMatrix) -> f32 {
    let a = &m.0;
    let g = |i: usize| a.get(i).copied().unwrap_or(0.0);
    let (m00, m01, m02) = (g(0), g(1), g(2));
    let (m10, m11, m12) = (g(4), g(5), g(6));
    let (m20, m21, m22) = (g(8), g(9), g(10));
    m00 * (m11 * m22 - m12 * m21) - m01 * (m10 * m22 - m12 * m20) + m02 * (m10 * m21 - m11 * m20)
}
