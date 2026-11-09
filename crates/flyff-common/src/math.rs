//! Left-handed (D3D) math helpers (Plan/01 §8, D4).
//!
//! Coordinates stay D3D left-handed (+Y up, +Z north) everywhere in Rust. `glam` is built with
//! `scalar-math` (no SIMD types) and `libm`; every transcendental function here goes through `libm`.

pub use glam::Vec3;

/// C `(int)` cast of a `float` with x86 `cvttss2si` semantics (Plan/31 §2.1 rule 6).
///
/// Truncates toward zero; a NaN or a value outside the `i32` range yields `i32::MIN` (the x86
/// "integer indefinite" value), unlike Rust's saturating `as` cast.
pub fn ftoi_x86(x: f32) -> i32 {
    // -2^31 and 2^31 are exactly representable; every f32 in [-2^31, 2^31) truncates into i32.
    if (-2_147_483_648.0_f32..2_147_483_648.0_f32).contains(&x) {
        x as i32 // BOUNDS: range checked above, NaN fails both comparisons
    } else {
        i32::MIN
    }
}

/// C `(int)` cast of a `double` with x86 `cvttsd2si` semantics (Plan/31 §2.1 rule 6).
///
/// Truncates toward zero; a NaN or a value whose truncation does not fit `i32` yields `i32::MIN`.
pub fn dtoi_x86(x: f64) -> i32 {
    // The truncated value fits iff -2^31 - 1 < x < 2^31.
    if x > -2_147_483_649.0_f64 && x < 2_147_483_648.0_f64 {
        x as i32 // BOUNDS: range checked above, NaN fails both comparisons
    } else {
        i32::MIN
    }
}

/// Row-vector 4x4 matrix with D3DX semantics (`v' = v · M`), stored row-major.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct D3dMatrix(pub [f32; 16]);

impl D3dMatrix {
    /// The identity matrix.
    pub const IDENTITY: D3dMatrix = D3dMatrix([
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]);

    /// Translation by `(x, y, z)` (`D3DXMatrixTranslation`).
    pub const fn translation(x: f32, y: f32, z: f32) -> D3dMatrix {
        D3dMatrix([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            x, y, z, 1.0,
        ])
    }

    /// Scaling (`D3DXMatrixScaling`).
    pub const fn scaling(x: f32, y: f32, z: f32) -> D3dMatrix {
        D3dMatrix([
            x, 0.0, 0.0, 0.0, //
            0.0, y, 0.0, 0.0, //
            0.0, 0.0, z, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Rotation about +Y by `rad` radians (`D3DXMatrixRotationY`, left-handed).
    pub fn rotation_y(rad: f32) -> D3dMatrix {
        let (s, c) = (libm::sinf(rad), libm::cosf(rad));
        D3dMatrix([
            c, 0.0, -s, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            s, 0.0, c, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Matrix product `self · rhs` (apply `self` first, then `rhs`, for row vectors).
    pub fn mul(&self, rhs: &D3dMatrix) -> D3dMatrix {
        let mut out = [0.0f32; 16];
        for r in 0..4 {
            for c in 0..4 {
                let mut acc = 0.0f32;
                for k in 0..4 {
                    acc += self.get(r, k) * rhs.get(k, c);
                }
                if let Some(slot) = out.get_mut(r * 4 + c) {
                    *slot = acc;
                }
            }
        }
        D3dMatrix(out)
    }

    /// Transforms a point (`D3DXVec3TransformCoord`, w assumed 1, no perspective divide).
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            p.x * self.get(0, 0) + p.y * self.get(1, 0) + p.z * self.get(2, 0) + self.get(3, 0),
            p.x * self.get(0, 1) + p.y * self.get(1, 1) + p.z * self.get(2, 1) + self.get(3, 1),
            p.x * self.get(0, 2) + p.y * self.get(1, 2) + p.z * self.get(2, 2) + self.get(3, 2),
        )
    }

    fn get(&self, row: usize, col: usize) -> f32 {
        self.0.get(row * 4 + col).copied().unwrap_or(0.0)
    }
}

impl Default for D3dMatrix {
    fn default() -> Self {
        D3dMatrix::IDENTITY
    }
}
