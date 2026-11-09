// The ONE place where the left-handed Direct3D world (server, sim, protocol: +Y up, +Z north, row vectors)
// is mapped into three.js space (Plan/41 section 3.1, Plan/01 section 8, Plan/30 section 12;
// `crates/flyff-formats/src/space.rs` is the Rust mirror and `tests/golden/space.json` the shared golden).
//
//   S = diag(1, 1, -1, 1)
//   positions, normals, event/force points, directions   (x, y, z)        -> (x, y, -z)
//   quaternions                                          (x, y, z, w)     -> (-x, -y, z, w)
//   matrices (row-vector D3D memory == column-major)     16 floats, negate indices 2, 6, 8, 9, 11, 14  (S*M*S)
//   AABB                                                 z: (-max.z, -min.z)
//   triangle winding                                     (a, b, c)        -> (a, c, b)   (done by the packs)
//   yaw                                                  rotation.y = +angle * pi / 180
//
// LAYERING (eslint/rules.js): this file may import three.js TYPES only (`import type`), nothing else of
// `render/`, so the boot path (input, audio) can use it without pulling in the lazy render chunk (Plan/40
// section 16.1). Every function here is pure and exact (negations and copies), except `yawToThree` and `det3`.

import type { Matrix4 } from 'three';

/** A 3-vector as plain numbers (LH or three.js space, depending on the caller). */
export type Vec3 = readonly [number, number, number];
/** A quaternion `(x, y, z, w)`. */
export type Quat = readonly [number, number, number, number];
/** The 16 floats of a 4x4 matrix in D3D row-major memory order (`_11 _12 _13 _14 _21 ...`). */
export type Mat16 = ArrayLike<number>;
/** A writable 16-float target (plain array or `Float32Array`). */
export interface Mat16Out {
  [index: number]: number;
  readonly length: number;
}

/** The matrix elements negated by the mirror `S * M * S` (`_13 _23 _31 _32 _34 _43`). */
export const MIRROR_NEGATED: readonly [number, number, number, number, number, number] = [
  2, 6, 8, 9, 11, 14,
];

/** `(x, y, z) -> (x, y, -z)`: positions, normals, event/force points, directions, paths. */
export const mirrorPos = (x: number, y: number, z: number): [number, number, number] => [x, y, -z];

/** {@link mirrorPos} of a tuple. */
export const mirrorVec3 = (v: Vec3): [number, number, number] => [v[0], v[1], -v[2]];

/** Negates the Z of every vertex of an interleaved `x, y, z` array in place (positions or normals). */
export function mirrorPositionsInPlace(a: Float32Array | number[]): void {
  for (let i = 2; i < a.length; i += 3) a[i] = -(a[i] ?? 0);
}

/** `(x, y, z, w) -> (-x, -y, z, w)`. */
export const mirrorQuat = (q: Quat): [number, number, number, number] => [-q[0], -q[1], q[2], q[3]];

/**
 * `S * M * S`: copies the 16 floats of `m` into `out` and negates indices 2, 6, 8, 9, 11 and 14. D3D
 * row-major/row-vector memory equals three.js column-major/column-vector memory for the same transform, so
 * the result is directly the `elements` of a three.js `Matrix4` (`Matrix4.fromArray`).
 */
export function mirrorMat(m: Mat16, out: Mat16Out): void {
  for (let i = 0; i < 16; i++) out[i] = m[i] ?? 0;
  for (const i of MIRROR_NEGATED) out[i] = -(out[i] ?? 0);
}

/** {@link mirrorMat} into a fresh array. */
export function mirrorMatrix(m: Mat16): number[] {
  const out = new Array<number>(16).fill(0);
  mirrorMat(m, out);
  return out;
}

/** The mirrored axis-aligned box: `z` becomes `(-max.z, -min.z)` so that `min <= max` still holds. */
export function mirrorAabb(
  min: Vec3,
  max: Vec3,
): [[number, number, number], [number, number, number]] {
  return [
    [min[0], min[1], -max[2]],
    [max[0], max[1], -min[2]],
  ];
}

/**
 * The inverse mapping for `SimWorld.ray_cast`: a three.js ray (origin, direction) into LH. The mirror is its
 * own inverse, so this is the same negation of `z`.
 */
export function rayToLh(o: Vec3, d: Vec3): [[number, number, number], [number, number, number]] {
  return [mirrorVec3(o), mirrorVec3(d)];
}

/**
 * The three.js `rotation.y` for a Flyff yaw `angle` in degrees: `+angle * pi / 180` (the world rotation is
 * `RotY(-angle)` in D3D row-vector form and `S * RotY(-a) * S = RotY(+a)`). Double precision, like the Rust
 * mirror, which rounds the result once to f32.
 */
export const yawToThree = (angleDeg: number): number => (angleDeg * Math.PI) / 180;

/**
 * Determinant of the upper-left 3x3 block, evaluated with f32 rounding after every operation so that the
 * result is bit-equal to `flyff_formats::space::det3` (mirrored-node detection: a negative value means the
 * node transform flips handedness, 29 corpus objects have one).
 */
export function det3(m: Mat16): number {
  const f = Math.fround;
  const g = (i: number): number => f(m[i] ?? 0);
  const m00 = g(0);
  const m01 = g(1);
  const m02 = g(2);
  const m10 = g(4);
  const m11 = g(5);
  const m12 = g(6);
  const m20 = g(8);
  const m21 = g(9);
  const m22 = g(10);
  const a = f(m00 * f(f(m11 * m22) - f(m12 * m21)));
  const b = f(m01 * f(f(m10 * m22) - f(m12 * m20)));
  const c = f(m02 * f(f(m10 * m21) - f(m11 * m20)));
  return f(f(a - b) + c);
}

/** Reverses the winding of every complete triangle `(a, b, c) -> (a, c, b)` in place. */
export function flipWinding(ib: Uint16Array | Uint32Array | number[]): void {
  for (let i = 0; i + 2 < ib.length; i += 3) {
    const t = ib[i + 1] ?? 0;
    ib[i + 1] = ib[i + 2] ?? 0;
    ib[i + 2] = t;
  }
}

/**
 * Three vec4 rows of the 3x4 transposed affine matrix (Plan/31 section 13.4: row `r` = `(c0[r], c1[r], c2[r],
 * c3[r])`, the columns of the three.js matrix, `c3` = translation) at `src[off .. off + 12]` into `out`.
 */
export function rows3ToMatrix4(src: ArrayLike<number>, off: number, out: Matrix4): void {
  const g = (i: number): number => src[off + i] ?? 0;
  // Matrix4.set takes its arguments in row-major order
  out.set(g(0), g(1), g(2), g(3), g(4), g(5), g(6), g(7), g(8), g(9), g(10), g(11), 0, 0, 0, 1);
}

/** The reverse of {@link rows3ToMatrix4}: writes the three top rows of `m` as 12 floats at `out[off ..]`. */
export function matrix4ToRows3(m: Matrix4, out: Mat16Out, off: number): void {
  const e = m.elements;
  for (let r = 0; r < 3; r++) {
    for (let c = 0; c < 4; c++) out[off + r * 4 + c] = e[c * 4 + r] ?? 0;
  }
}
