// Allocation-free row-vector affine helpers for the SFX math (Plan/41 section 16.4, ref/20 section 4.1).
//
// The SFX parts are evaluated in the left-handed D3D convention with row vectors (`v' = v * M`, the matrices compose left to
// right), exactly like the original; the finished matrix is mapped into three.js space ONCE by the quad batcher (`z` of the
// rows and of the translation is negated, `S * M * S`). `render/d3dx.ts` has the generic 4x4 versions (used by the tests as
// the reference); this file keeps the per-particle path free of allocations.
//
// An `Aff` is 12 numbers: three rows of the 3x3 part (`r0 r1 r2`) followed by the translation row `t`. `v' = v * R + t`.

/** Row-vector affine matrix: `[r0x r0y r0z, r1x r1y r1z, r2x r2y r2z, tx ty tz]`. */
export type Aff = Float64Array;

export const newAff = (): Aff => new Float64Array(12);

/** `degrees -> radians` (`DEGREETORADIAN`). */
export const RAD = Math.PI / 180;

export function setIdentity(o: Aff): Aff {
  o[0] = 1;
  o[1] = 0;
  o[2] = 0;
  o[3] = 0;
  o[4] = 1;
  o[5] = 0;
  o[6] = 0;
  o[7] = 0;
  o[8] = 1;
  o[9] = 0;
  o[10] = 0;
  o[11] = 0;
  return o;
}

/** `D3DXMatrixRotationX`. */
export function setRotX(o: Aff, rad: number): Aff {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  setIdentity(o);
  o[4] = c;
  o[5] = s;
  o[7] = -s;
  o[8] = c;
  return o;
}

/** `D3DXMatrixRotationY`. */
export function setRotY(o: Aff, rad: number): Aff {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  setIdentity(o);
  o[0] = c;
  o[2] = -s;
  o[6] = s;
  o[8] = c;
  return o;
}

/** `D3DXMatrixRotationZ`. */
export function setRotZ(o: Aff, rad: number): Aff {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  setIdentity(o);
  o[0] = c;
  o[1] = s;
  o[3] = -s;
  o[4] = c;
  return o;
}

export function setScale(o: Aff, x: number, y: number, z: number): Aff {
  setIdentity(o);
  o[0] = x;
  o[4] = y;
  o[8] = z;
  return o;
}

/**
 * `D3DXMatrixRotationYawPitchRoll(yaw, pitch, roll)` = `Rz(roll) * Rx(pitch) * Ry(yaw)` in row-vector order, in the closed form
 * of the D3DX source (angles in radians).
 */
export function setYpr(o: Aff, yaw: number, pitch: number, roll: number): Aff {
  const cy = Math.cos(yaw);
  const sy = Math.sin(yaw);
  const cp = Math.cos(pitch);
  const sp = Math.sin(pitch);
  const cr = Math.cos(roll);
  const sr = Math.sin(roll);
  o[0] = cr * cy + sr * sp * sy;
  o[1] = sr * cp;
  o[2] = cr * -sy + sr * sp * cy;
  o[3] = -sr * cy + cr * sp * sy;
  o[4] = cr * cp;
  o[5] = sr * sy + cr * sp * cy;
  o[6] = cp * sy;
  o[7] = -sp;
  o[8] = cp * cy;
  o[9] = 0;
  o[10] = 0;
  o[11] = 0;
  return o;
}

/** `YPR` of a degrees vector `(x = pitch, y = yaw, z = roll)`, the `D3DXMatrixRotationYawPitchRoll(v.y, v.x, v.z)` of the source. */
export function setYprDeg(o: Aff, rx: number, ry: number, rz: number): Aff {
  return setYpr(o, ry * RAD, rx * RAD, rz * RAD);
}

/** `o = a * b` (`a` applies first). `o` must not be `a` or `b`. */
export function mul(o: Aff, a: Aff, b: Aff): Aff {
  const a0 = a[0] ?? 0;
  const a1 = a[1] ?? 0;
  const a2 = a[2] ?? 0;
  const a3 = a[3] ?? 0;
  const a4 = a[4] ?? 0;
  const a5 = a[5] ?? 0;
  const a6 = a[6] ?? 0;
  const a7 = a[7] ?? 0;
  const a8 = a[8] ?? 0;
  const at0 = a[9] ?? 0;
  const at1 = a[10] ?? 0;
  const at2 = a[11] ?? 0;
  const b0 = b[0] ?? 0;
  const b1 = b[1] ?? 0;
  const b2 = b[2] ?? 0;
  const b3 = b[3] ?? 0;
  const b4 = b[4] ?? 0;
  const b5 = b[5] ?? 0;
  const b6 = b[6] ?? 0;
  const b7 = b[7] ?? 0;
  const b8 = b[8] ?? 0;
  o[0] = a0 * b0 + a1 * b3 + a2 * b6;
  o[1] = a0 * b1 + a1 * b4 + a2 * b7;
  o[2] = a0 * b2 + a1 * b5 + a2 * b8;
  o[3] = a3 * b0 + a4 * b3 + a5 * b6;
  o[4] = a3 * b1 + a4 * b4 + a5 * b7;
  o[5] = a3 * b2 + a4 * b5 + a5 * b8;
  o[6] = a6 * b0 + a7 * b3 + a8 * b6;
  o[7] = a6 * b1 + a7 * b4 + a8 * b7;
  o[8] = a6 * b2 + a7 * b5 + a8 * b8;
  o[9] = at0 * b0 + at1 * b3 + at2 * b6 + (b[9] ?? 0);
  o[10] = at0 * b1 + at1 * b4 + at2 * b7 + (b[10] ?? 0);
  o[11] = at0 * b2 + at1 * b5 + at2 * b8 + (b[11] ?? 0);
  return o;
}

/** Writes `(x, y, z, 1) * m` to `out[0..2]`. */
export function transform(
  out: number[] | Float64Array,
  x: number,
  y: number,
  z: number,
  m: Aff,
): void {
  out[0] = x * (m[0] ?? 0) + y * (m[3] ?? 0) + z * (m[6] ?? 0) + (m[9] ?? 0);
  out[1] = x * (m[1] ?? 0) + y * (m[4] ?? 0) + z * (m[7] ?? 0) + (m[10] ?? 0);
  out[2] = x * (m[2] ?? 0) + y * (m[5] ?? 0) + z * (m[8] ?? 0) + (m[11] ?? 0);
}

/** `D3DXVec3TransformNormal`-style: the 3x3 part only. */
export function transformDir(
  out: number[] | Float64Array,
  x: number,
  y: number,
  z: number,
  m: Aff,
): void {
  out[0] = x * (m[0] ?? 0) + y * (m[3] ?? 0) + z * (m[6] ?? 0);
  out[1] = x * (m[1] ?? 0) + y * (m[4] ?? 0) + z * (m[7] ?? 0);
  out[2] = x * (m[2] ?? 0) + y * (m[5] ?? 0) + z * (m[8] ?? 0);
}

/** An `Aff` from the 16 numbers of a D3D row-major matrix (the last column is assumed to be `0 0 0 1`). */
export function fromMat16(o: Aff, m: ArrayLike<number>): Aff {
  o[0] = m[0] ?? 0;
  o[1] = m[1] ?? 0;
  o[2] = m[2] ?? 0;
  o[3] = m[4] ?? 0;
  o[4] = m[5] ?? 0;
  o[5] = m[6] ?? 0;
  o[6] = m[8] ?? 0;
  o[7] = m[9] ?? 0;
  o[8] = m[10] ?? 0;
  o[9] = m[12] ?? 0;
  o[10] = m[13] ?? 0;
  o[11] = m[14] ?? 0;
  return o;
}

/** The 16 numbers of the D3D row-major matrix of an `Aff` (tests). */
export function toMat16(m: Aff): number[] {
  return [
    m[0] ?? 0,
    m[1] ?? 0,
    m[2] ?? 0,
    0,
    m[3] ?? 0,
    m[4] ?? 0,
    m[5] ?? 0,
    0,
    m[6] ?? 0,
    m[7] ?? 0,
    m[8] ?? 0,
    0,
    m[9] ?? 0,
    m[10] ?? 0,
    m[11] ?? 0,
    1,
  ];
}
