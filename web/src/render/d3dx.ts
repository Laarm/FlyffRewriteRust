// LH row-vector D3DX helpers for render math (Plan/41 section 3.2). The original evaluates SFX `YPR`, the sky, the
// lens flare, the shadow map and the yo-yo path in the left-handed D3D convention; the renderer does the same
// with these helpers and maps the FINAL matrix to three.js once (`toThree` = `mirrorMat`, S*M*S).
//
// Matrices are 16 numbers in D3D row-major memory order (`_11 _12 _13 _14 _21 ...`), vectors are row vectors
// (`v' = v * M`), so `multiply(a, b)` applies `a` first. f32 rounding is not reproduced (visual only).
// Formulas are the documented D3DX ones; ref/20 section 4 and Plan/31 section 13.3 list the users.

import { mirrorMatrix, type Vec3 } from './space';

/** A 4x4 matrix in D3D row-major memory order. */
export type Mat4 = number[];

export const identity = (): Mat4 => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

/** `D3DXMatrixRotationX`: `_22 = c, _23 = s, _32 = -s, _33 = c`. */
export function rotationX(rad: number): Mat4 {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1];
}

/** `D3DXMatrixRotationY`: `_11 = c, _13 = -s, _31 = s, _33 = c`. */
export function rotationY(rad: number): Mat4 {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1];
}

/** `D3DXMatrixRotationZ`: `_11 = c, _12 = s, _21 = -s, _22 = c`. */
export function rotationZ(rad: number): Mat4 {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return [c, s, 0, 0, -s, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
}

export const scaling = (x: number, y: number, z: number): Mat4 => [
  x,
  0,
  0,
  0,
  0,
  y,
  0,
  0,
  0,
  0,
  z,
  0,
  0,
  0,
  0,
  1,
];

export const translation = (x: number, y: number, z: number): Mat4 => [
  1,
  0,
  0,
  0,
  0,
  1,
  0,
  0,
  0,
  0,
  1,
  0,
  x,
  y,
  z,
  1,
];

/** `a * b` in row-vector order: a vector is transformed by `a` first, then by `b`. */
export function multiply(a: readonly number[], b: readonly number[]): Mat4 {
  const out = new Array<number>(16);
  for (let r = 0; r < 4; r++) {
    const a0 = a[r * 4] ?? 0;
    const a1 = a[r * 4 + 1] ?? 0;
    const a2 = a[r * 4 + 2] ?? 0;
    const a3 = a[r * 4 + 3] ?? 0;
    for (let c = 0; c < 4; c++) {
      out[r * 4 + c] =
        a0 * (b[c] ?? 0) + a1 * (b[4 + c] ?? 0) + a2 * (b[8 + c] ?? 0) + a3 * (b[12 + c] ?? 0);
    }
  }
  return out;
}

/** `D3DXMatrixRotationYawPitchRoll(yaw, pitch, roll)` = `Rz(roll) * Rx(pitch) * Ry(yaw)` (row vectors). */
export function yawPitchRoll(yaw: number, pitch: number, roll: number): Mat4 {
  return multiply(multiply(rotationZ(roll), rotationX(pitch)), rotationY(yaw));
}

/** `D3DXVec3TransformCoord`: `(v, 1) * m`, divided by w. */
export function transformCoord(v: Vec3, m: readonly number[]): [number, number, number] {
  const x = v[0];
  const y = v[1];
  const z = v[2];
  const ox = x * (m[0] ?? 0) + y * (m[4] ?? 0) + z * (m[8] ?? 0) + (m[12] ?? 0);
  const oy = x * (m[1] ?? 0) + y * (m[5] ?? 0) + z * (m[9] ?? 0) + (m[13] ?? 0);
  const oz = x * (m[2] ?? 0) + y * (m[6] ?? 0) + z * (m[10] ?? 0) + (m[14] ?? 0);
  const ow = x * (m[3] ?? 0) + y * (m[7] ?? 0) + z * (m[11] ?? 0) + (m[15] ?? 0);
  const iw = ow !== 0 ? 1 / ow : 1;
  return [ox * iw, oy * iw, oz * iw];
}

/** `D3DXVec3TransformNormal`: `(v, 0) * m` (no translation, no divide). */
export function transformNormal(v: Vec3, m: readonly number[]): [number, number, number] {
  const x = v[0];
  const y = v[1];
  const z = v[2];
  return [
    x * (m[0] ?? 0) + y * (m[4] ?? 0) + z * (m[8] ?? 0),
    x * (m[1] ?? 0) + y * (m[5] ?? 0) + z * (m[9] ?? 0),
    x * (m[2] ?? 0) + y * (m[6] ?? 0) + z * (m[10] ?? 0),
  ];
}

const sub = (a: Vec3, b: Vec3): [number, number, number] => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: Vec3, b: Vec3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): [number, number, number] => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
function normalize(v: Vec3): [number, number, number] {
  const l = Math.hypot(v[0], v[1], v[2]);
  return l > 0 ? [v[0] / l, v[1] / l, v[2] / l] : [0, 0, 0];
}

/**
 * `D3DXMatrixLookAtLH(eye, at, up)`: `z = normalize(at - eye)`, `x = normalize(up x z)`, `y = z x x`;
 * columns `x y z`, translation row `(-x.eye, -y.eye, -z.eye)`.
 */
export function lookAtLH(eye: Vec3, at: Vec3, up: Vec3): Mat4 {
  const z = normalize(sub(at, eye));
  const x = normalize(cross(up, z));
  const y = cross(z, x);
  return [
    x[0],
    y[0],
    z[0],
    0,
    x[1],
    y[1],
    z[1],
    0,
    x[2],
    y[2],
    z[2],
    0,
    -dot(x, eye),
    -dot(y, eye),
    -dot(z, eye),
    1,
  ];
}

/**
 * `D3DXMatrixPerspectiveFovLH(fovY, aspect, zn, zf)`: clip z in [0, 1] (the GL projection of three.js has
 * [-1, 1]; the visible result is identical).
 */
export function perspectiveFovLH(fovY: number, aspect: number, zn: number, zf: number): Mat4 {
  const yScale = 1 / Math.tan(fovY / 2);
  const xScale = yScale / aspect;
  const q = zf / (zf - zn);
  return [xScale, 0, 0, 0, 0, yScale, 0, 0, 0, 0, q, 1, 0, 0, -zn * q, 0];
}

/** General 4x4 inverse by cofactors; a singular matrix returns the identity (as `inverse4x4` of Plan/31). */
export function inverse(m: readonly number[]): Mat4 {
  const a = (i: number): number => m[i] ?? 0;
  const s0 = a(0) * a(5) - a(4) * a(1);
  const s1 = a(0) * a(6) - a(4) * a(2);
  const s2 = a(0) * a(7) - a(4) * a(3);
  const s3 = a(1) * a(6) - a(5) * a(2);
  const s4 = a(1) * a(7) - a(5) * a(3);
  const s5 = a(2) * a(7) - a(6) * a(3);
  const c5 = a(10) * a(15) - a(14) * a(11);
  const c4 = a(9) * a(15) - a(13) * a(11);
  const c3 = a(9) * a(14) - a(13) * a(10);
  const c2 = a(8) * a(15) - a(12) * a(11);
  const c1 = a(8) * a(14) - a(12) * a(10);
  const c0 = a(8) * a(13) - a(12) * a(9);
  const det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
  if (Math.abs(det) < 1e-12) return identity();
  const id = 1 / det;
  return [
    (a(5) * c5 - a(6) * c4 + a(7) * c3) * id,
    (-a(1) * c5 + a(2) * c4 - a(3) * c3) * id,
    (a(13) * s5 - a(14) * s4 + a(15) * s3) * id,
    (-a(9) * s5 + a(10) * s4 - a(11) * s3) * id,
    (-a(4) * c5 + a(6) * c2 - a(7) * c1) * id,
    (a(0) * c5 - a(2) * c2 + a(3) * c1) * id,
    (-a(12) * s5 + a(14) * s2 - a(15) * s1) * id,
    (a(8) * s5 - a(10) * s2 + a(11) * s1) * id,
    (a(4) * c4 - a(5) * c2 + a(7) * c0) * id,
    (-a(0) * c4 + a(1) * c2 - a(3) * c0) * id,
    (a(12) * s4 - a(13) * s2 + a(15) * s0) * id,
    (-a(8) * s4 + a(9) * s2 - a(11) * s0) * id,
    (-a(4) * c3 + a(5) * c1 - a(6) * c0) * id,
    (a(0) * c3 - a(1) * c1 + a(2) * c0) * id,
    (-a(12) * s3 + a(13) * s1 - a(14) * s0) * id,
    (a(8) * s3 - a(9) * s1 + a(10) * s0) * id,
  ];
}

/** `D3DXQuaternionRotationAxis(axis, angle)`: `(normalize(axis) * sin(a/2), cos(a/2))`. */
export function quaternionRotationAxis(axis: Vec3, rad: number): [number, number, number, number] {
  const n = normalize(axis);
  const s = Math.sin(rad / 2);
  return [n[0] * s, n[1] * s, n[2] * s, Math.cos(rad / 2)];
}

/** `D3DXMatrixRotationQuaternion(q)` (row-vector layout; Plan/31 section 13.3). */
export function rotationQuaternion(q: readonly [number, number, number, number]): Mat4 {
  const [x, y, z, w] = q;
  return [
    1 - 2 * (y * y + z * z),
    2 * (x * y + z * w),
    2 * (x * z - y * w),
    0,
    2 * (x * y - z * w),
    1 - 2 * (x * x + z * z),
    2 * (y * z + x * w),
    0,
    2 * (x * z + y * w),
    2 * (y * z - x * w),
    1 - 2 * (x * x + y * y),
    0,
    0,
    0,
    0,
    1,
  ];
}

/**
 * `D3DXQuaternionRotationMatrix(m)` (render-audio-m7; the Wine / D3DX algorithm): NOT normalising, so a matrix that keeps its model scale (the
 * ClockWorks cannon's `world * bone 9`, `CSfxClockWorksCannon::Process`) gives the quaternion the original gets from it.
 */
export function quaternionRotationMatrix(m: readonly number[]): [number, number, number, number] {
  const at = (r: number, c: number): number => m[r * 4 + c] ?? 0;
  const trace = at(0, 0) + at(1, 1) + at(2, 2) + 1;
  if (trace > 1) {
    const d = 2 * Math.sqrt(trace);
    return [
      (at(1, 2) - at(2, 1)) / d,
      (at(2, 0) - at(0, 2)) / d,
      (at(0, 1) - at(1, 0)) / d,
      Math.sqrt(trace) / 2,
    ];
  }
  let maxi = 0;
  let maxdiag = at(0, 0);
  for (let i = 1; i < 3; i++) {
    if (at(i, i) > maxdiag) {
      maxi = i;
      maxdiag = at(i, i);
    }
  }
  if (maxi === 0) {
    const s = 2 * Math.sqrt(1 + at(0, 0) - at(1, 1) - at(2, 2));
    return [
      0.25 * s,
      (at(0, 1) + at(1, 0)) / s,
      (at(0, 2) + at(2, 0)) / s,
      (at(1, 2) - at(2, 1)) / s,
    ];
  }
  if (maxi === 1) {
    const s = 2 * Math.sqrt(1 + at(1, 1) - at(0, 0) - at(2, 2));
    return [
      (at(0, 1) + at(1, 0)) / s,
      0.25 * s,
      (at(1, 2) + at(2, 1)) / s,
      (at(2, 0) - at(0, 2)) / s,
    ];
  }
  const s = 2 * Math.sqrt(1 + at(2, 2) - at(0, 0) - at(1, 1));
  return [
    (at(0, 2) + at(2, 0)) / s,
    (at(1, 2) + at(2, 1)) / s,
    0.25 * s,
    (at(0, 1) - at(1, 0)) / s,
  ];
}

/** A viewport rectangle in pixels with its depth range. */
export interface Viewport {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly minZ: number;
  readonly maxZ: number;
}

/**
 * `D3DXVec3Project(v, viewport, proj, view, world)`: the point through `world * view * proj`, then to window
 * coordinates (y down).
 */
export function vec3Project(
  v: Vec3,
  viewport: Viewport,
  proj: readonly number[],
  view: readonly number[],
  world: readonly number[] = identity(),
): [number, number, number] {
  const wvp = multiply(multiply(world, view), proj);
  const [x, y, z] = transformCoord(v, wvp);
  return [
    viewport.x + ((1 + x) * viewport.width) / 2,
    viewport.y + ((1 - y) * viewport.height) / 2,
    viewport.minZ + z * (viewport.maxZ - viewport.minZ),
  ];
}

/** Maps a finished LH matrix into three.js space (`S * M * S`, the elements of a column-major `Matrix4`). */
export const toThree = (mLH: readonly number[]): number[] => mirrorMatrix(mLH);

/** Degrees to radians. */
export const rad = (deg: number): number => (deg * Math.PI) / 180;
