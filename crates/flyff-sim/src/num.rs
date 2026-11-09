//! Numeric helpers and exact transcriptions of the original angle/vector functions
//! (Plan/31 §2, §2.3; WP-SIM-01).
//!
//! Every operation is written component-wise in the order of the original C++ expression so that the
//! result is reproducible bit for bit on every target: no FMA contraction (Rust never fuses `a*b+c`),
//! transcendentals only through `libm`, `f32` where the original used `float`, `f64` where it used
//! `double` (the `SetAngle` quantisation). No function here produces NaN from finite input.

pub use flyff_common::math::{Vec3, dtoi_x86, ftoi_x86};

/// `D3DXToRadian(d) = d * (D3DX_PI / 180)` with the `float` constant `D3DX_PI = 3.141592654f`.
pub const DEG2RAD: f32 = core::f32::consts::PI / 180.0_f32;
/// `D3DXToDegree(r) = r * (180 / D3DX_PI)`.
pub const RAD2DEG: f32 = 180.0_f32 / core::f32::consts::PI;

/// `sinf` (libm, deterministic on every target).
#[inline]
pub fn sinf(x: f32) -> f32 {
    libm::sinf(x)
}

/// `cosf` (libm).
#[inline]
pub fn cosf(x: f32) -> f32 {
    libm::cosf(x)
}

/// `acosf` (libm); callers clamp the argument to `[-1, 1]`.
#[inline]
pub fn acosf(x: f32) -> f32 {
    libm::acosf(x)
}

/// `atan2f` (libm).
#[inline]
pub fn atan2f(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

/// `sqrtf`: IEEE-754 square root, exact on every target.
#[inline]
pub fn sqrtf(x: f32) -> f32 {
    libm::sqrtf(x)
}

/// `D3DXToRadian`.
#[inline]
pub fn to_radian(deg: f32) -> f32 {
    deg * DEG2RAD
}

/// `D3DXToDegree`.
#[inline]
pub fn to_degree(rad: f32) -> f32 {
    rad * RAD2DEG
}

/// `D3DXVec3Dot`: `((ax*bx) + (ay*by)) + (az*bz)` evaluated left to right.
#[inline]
pub fn dot(a: Vec3, b: Vec3) -> f32 {
    (a.x * b.x + a.y * b.y) + a.z * b.z
}

/// `D3DXVec3Cross`.
#[inline]
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// `D3DXVec3LengthSq`.
#[inline]
pub fn length_sq(a: Vec3) -> f32 {
    dot(a, a)
}

/// `D3DXVec3Length` (`sqrtf` of the squared length).
#[inline]
pub fn length(a: Vec3) -> f32 {
    sqrtf(length_sq(a))
}

/// `D3DXVec3Normalize` with the D3DX zero-vector rule: a vector whose length is `0` (or not a
/// number) normalises to `(0, 0, 0)` (Plan/31 §2.1 rule 5). Components are divided by the length.
#[inline]
pub fn normalize_or_zero(v: Vec3) -> Vec3 {
    let l = length(v);
    if l == 0.0 || l.is_nan() {
        Vec3::ZERO
    } else {
        Vec3::new(v.x / l, v.y / l, v.z / l)
    }
}

/// `CObj::SetAngle` quantisation (`src:_Common/Obj.cpp:662`): computed in `f64`, KEEP-QUIRK KQ-13
/// (1/65536 of a turn). The result lies in `[0, 360)`; `NaN`, infinities and values beyond the `i32`
/// range map through the x86 "integer indefinite" (`i32::MIN & 0xFFFF == 0`) to `0.0`.
pub fn set_angle_quantized(a: f32) -> f32 {
    let q = dtoi_x86(f64::from(a) * (65536.0_f64 / 360.0_f64)) & 0xFFFF;
    ((360.0_f64 / 65536.0_f64) * f64::from(q)) as f32 // BOUNDS: result in [0, 360)
}

/// `CObj::AddAngle`: `+d` turns left. Returns the new quantised yaw.
#[inline]
pub fn add_angle(angle: f32, d: f32) -> f32 {
    set_angle_quantized(angle + d)
}

/// `VelocityToVec(angle, len)`: `(sin a, 0, -cos a) * len`.
#[inline]
pub fn velocity_to_vec(angle_deg: f32, len: f32) -> Vec3 {
    let t = to_radian(angle_deg);
    Vec3::new(sinf(t) * len, 0.0, -cosf(t) * len)
}

/// `GetDegree(dest, src)` (`Obj.h:288`): the XZ heading from `src` to `dest` in degrees, `[0, 360]`.
/// **FIX**: the `acosf` argument is clamped to `[-1, 1]` (the original could return NaN from rounding).
pub fn get_degree(dest: Vec3, src: Vec3) -> f32 {
    let d2 = normalize_or_zero(Vec3::new(dest.x - src.x, 0.0, dest.z - src.z));
    // vDir1 = normalize(VelocityToVec(0, 1)) = (0, 0, -1): dot = ((0*x) + (0*y)) + (-1*z)
    let dot = (0.0_f32 * d2.x + 0.0_f32 * d2.y) + -d2.z;
    let deg = to_degree(acosf(dot.clamp(-1.0, 1.0)));
    if d2.x < 0.0 { 360.0 - deg } else { deg }
}

/// `GetDegreeX(dest, src)` (`Obj.h:306`): pitch toward `dest`, negative when `dest` is higher.
pub fn get_degree_x(dest: Vec3, src: Vec3) -> f32 {
    let v = Vec3::new(dest.x - src.x, 0.0, dest.z - src.z);
    let y_dist = dest.y - src.y;
    let len_xz = length(v);
    let r = atan2f(y_dist, -len_xz);
    let a = -to_degree(r);
    if a > 90.0 {
        180.0 - a
    } else if a < -90.0 {
        -180.0 - a
    } else {
        a
    }
}

/// `AngleToVector(v, yaw, h, s)` (`xUtil3D.h:25`): `yaw` and `h` in degrees.
pub fn angle_to_vector(yaw_deg: f32, h_deg: f32, s: f32) -> Vec3 {
    let yaw = to_radian(yaw_deg);
    let h = to_radian(h_deg);
    let dist = cosf(-h) * s;
    Vec3::new(sinf(yaw) * dist, -sinf(-h) * s, -cosf(yaw) * dist)
}

/// `AngleToVectorXZ(yaw, s)` (`xUtil3D.h:37`): `(sin yaw * s, 0, -cos yaw * s)`.
pub fn angle_to_vector_xz(yaw_deg: f32, s: f32) -> Vec3 {
    let yaw = to_radian(yaw_deg);
    Vec3::new(sinf(yaw) * s, 0.0, -cosf(yaw) * s)
}

/// `xGetDegree(v)` (`xUtil3D.h:46`): `(yaw, pitch)` of a direction vector in degrees.
pub fn x_get_degree(v: Vec3) -> (f32, f32) {
    let yaw = to_degree(atan2f(v.x, -v.z));
    let len_xz = length(Vec3::new(v.x, 0.0, v.z));
    let pitch = to_degree(atan2f(len_xz, v.y)) - 90.0;
    (yaw, pitch)
}

/// `trunc3(p) = (float)((int)(p * 1000)) / 1000` (`ModelObject.cpp:582`): the `.ani` `perSlerp`
/// truncation to three decimals. Not idempotent in `f32`, so it must be applied exactly once.
#[inline]
pub fn trunc3(p: f32) -> f32 {
    (ftoi_x86(p * 1000.0) as f32) / 1000.0 // BOUNDS: i32 -> f32 is exact enough by design (original cast)
}
