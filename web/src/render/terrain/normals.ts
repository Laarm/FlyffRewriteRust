// Terrain vertex normals (Plan/41 section 9.2 **FIX** X7 / PL-133): over the GLOBAL height field, so that the two
// vertices that share a position on a land border get the same normal and the lighting shows no seam.
//
// The formula is the original's (ref/19 section 16.2) with its flaw removed: the sum of the four cross products of
// consecutive edges to the left / up / right / down neighbours (`e = neighbour - centre` in LH world units):
//
//     N = normalize( cross(eUp, eRight) + cross(eRight, eDown) + cross(eDown, eLeft) + cross(eLeft, eUp) )
//
// (flat ground gives +Y). The original skipped neighbours outside the land (index 0 and 128), which made the border
// normals one-sided and produced a lighting seam at every land border; here a neighbour in the adjacent land is used
// when that land is resident. A neighbour that does not exist (world edge, land not loaded) drops the terms that need
// it. "Up" is +z (north), "right" is +x (east).
//
// The result is stored in THREE.JS space (`z` mirrored) as the vertex attribute.

import { VERTS } from './landData';

/** Height of vertex `(vx, vz)` of the land being computed; `vx`/`vz` reach `-1` and `129`. `undefined` = no neighbour. */
export type HeightLookup = (vx: number, vz: number) => number | undefined;

/**
 * Writes the normal of vertex `(vx, vz)` into `out[at..at+2]` (three.js space). `mpu` is the horizontal size of a
 * cell in world units.
 */
export function vertexNormal(
  height: HeightLookup,
  vx: number,
  vz: number,
  mpu: number,
  out: Float32Array,
  at: number,
): void {
  const c = height(vx, vz) ?? 0;
  const l = height(vx - 1, vz);
  const r = height(vx + 1, vz);
  const u = height(vx, vz + 1);
  const d = height(vx, vz - 1);
  // edges to the neighbours: left (-m, dl, 0), up (0, du, m), right (m, dr, 0), down (0, dd, -m)
  let nx = 0;
  let ny = 0;
  let nz = 0;
  const add = (ax: number, ay: number, az: number, bx: number, by: number, bz: number): void => {
    nx += ay * bz - az * by;
    ny += az * bx - ax * bz;
    nz += ax * by - ay * bx;
  };
  const m = mpu;
  if (u !== undefined && r !== undefined) add(0, u - c, m, m, r - c, 0);
  if (r !== undefined && d !== undefined) add(m, r - c, 0, 0, d - c, -m);
  if (d !== undefined && l !== undefined) add(0, d - c, -m, -m, l - c, 0);
  if (l !== undefined && u !== undefined) add(-m, l - c, 0, 0, u - c, m);
  const len = Math.hypot(nx, ny, nz);
  if (len > 0) {
    nx /= len;
    ny /= len;
    nz /= len;
  } else {
    nx = 0;
    ny = 1;
    nz = 0;
  }
  out[at] = nx;
  out[at + 1] = ny;
  out[at + 2] = -nz;
}

/** Computes all 129 x 129 normals of a land (three.js space). */
export function computeNormals(
  height: HeightLookup,
  mpu: number,
  out?: Float32Array,
): Float32Array {
  const normals = out ?? new Float32Array(VERTS * VERTS * 3);
  for (let vz = 0; vz < VERTS; vz++) {
    for (let vx = 0; vx < VERTS; vx++) {
      vertexNormal(height, vx, vz, mpu, normals, (vz * VERTS + vx) * 3);
    }
  }
  return normals;
}

/**
 * Recomputes the border rows and columns (vertices 0 and 128 on each axis) after a neighbouring land loaded or
 * unloaded; interior normals never depend on the neighbours. Returns the number of vertices written.
 */
export function recomputeBorderNormals(
  height: HeightLookup,
  mpu: number,
  normals: Float32Array,
): number {
  let n = 0;
  for (let i = 0; i < VERTS; i++) {
    for (const [vx, vz] of [
      [i, 0],
      [i, VERTS - 1],
      [0, i],
      [VERTS - 1, i],
    ] as const) {
      vertexNormal(height, vx, vz, mpu, normals, (vz * VERTS + vx) * 3);
      n++;
    }
  }
  return n;
}
