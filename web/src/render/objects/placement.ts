// Placement of static objects (Plan/41 section 11.2, Plan/30 section 7.3, Obj.cpp:126-147 `__VER >= 11`).
//
//   World_LH = Scale(scale) * RotationYawPitchRoll(yaw = -angle deg, pitch = -angX deg, roll = +angZ deg) * Translate(pos)
//
// in the D3DX row-vector convention (`d3dx.ts`; `RotationYawPitchRoll = Rz(roll) * Rx(pitch) * Ry(yaw)`), then ONE
// `mirrorMat` (S * M * S) into three.js space. The `.lnd` record carries `(angle, angX, angZ)` as written in the file; the
// signs are applied here. The static object's model hierarchy (`localTM` of the GMOBJECTs, already mapped to three.js
// space by the decoder) is applied FIRST: `M_total = M_world * M_local` in three.js column-vector terms.

import { Matrix4, Vector3, type Box3 } from 'three';
import { mirrorMatrix } from '../space';
import { multiply, scaling, translation, yawPitchRoll } from '../d3dx';

const RAD = Math.PI / 180;

/** The record of one placement (`LndMeta.objects.xf`: `pos xyz, angle, angX, angZ, scale xyz, 0`). */
export interface Placement {
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  readonly angX: number;
  readonly angZ: number;
  readonly scale: readonly [number, number, number];
}

/** Reads placement `i` of a land's `xf` section. */
export function placementAt(xf: Float32Array, i: number): Placement {
  const o = i * 10;
  return {
    pos: [xf[o] ?? 0, xf[o + 1] ?? 0, xf[o + 2] ?? 0],
    angle: xf[o + 3] ?? 0,
    angX: xf[o + 4] ?? 0,
    angZ: xf[o + 5] ?? 0,
    scale: [xf[o + 6] ?? 1, xf[o + 7] ?? 1, xf[o + 8] ?? 1],
  };
}

/** The LH world matrix in D3D row-major memory order (16 numbers). */
export function worldMatrixLH(p: Placement): number[] {
  const s = scaling(p.scale[0], p.scale[1], p.scale[2]);
  const r = yawPitchRoll(-p.angle * RAD, -p.angX * RAD, p.angZ * RAD);
  const t = translation(p.pos[0], p.pos[1], p.pos[2]);
  return multiply(multiply(s, r), t);
}

/** The three.js-space world matrix of a placement (column-major elements), written into `out`. */
export function worldMatrixThree(p: Placement, out: Matrix4 = new Matrix4()): Matrix4 {
  return out.fromArray(mirrorMatrix(worldMatrixLH(p)));
}

const TMP = new Vector3();

/** The centre and radius of the bounding sphere of `bbox` (model space) under `matrix` (three.js space). */
export function worldSphere(
  bbox: Box3,
  matrix: Matrix4,
  out: { center: Vector3; radius: number },
): void {
  bbox.getCenter(out.center).applyMatrix4(matrix);
  // the radius: half the box diagonal times the largest column scale of the matrix
  const e = matrix.elements;
  const sx = Math.hypot(e[0], e[1], e[2]);
  const sy = Math.hypot(e[4], e[5], e[6]);
  const sz = Math.hypot(e[8], e[9], e[10]);
  bbox.getSize(TMP);
  out.radius = 0.5 * TMP.length() * Math.max(sx, sy, sz);
}

/** Fills 12 floats of an instance buffer: three columns `(x, y, z, translation)` of `matrix`. */
export function writeInstance(matrix: Matrix4, out: Float32Array, at: number): void {
  const e = matrix.elements;
  out[at] = e[0];
  out[at + 1] = e[1];
  out[at + 2] = e[2];
  out[at + 3] = e[12];
  out[at + 4] = e[4];
  out[at + 5] = e[5];
  out[at + 6] = e[6];
  out[at + 7] = e[13];
  out[at + 8] = e[8];
  out[at + 9] = e[9];
  out[at + 10] = e[10];
  out[at + 11] = e[14];
}
