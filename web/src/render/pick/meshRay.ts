// Triangle ray tests of the precise pick (Plan/41 section 20.3; `IntersectRayTri`, Object3D.cpp:2326-2457): a ray against the
// LOD-0 mesh of an object. Movers use the CURRENT pose approximated with the primary bone: every vertex goes through the
// palette matrix of its first joint only (no second weight), then through the world matrix; ctrls and statics are rigid.
// Front faces count unless the block is two-sided (both windings, `XE_2SIDE`); the winding is the three.js one (the decoder
// flipped it with the mirror), so a counter-clockwise triangle seen from its normal side is a front face.
//
// Pure functions over typed arrays (no scene, no GL): the mesh data is the CPU copy `ObjectGeometry.pick` plus the joint
// attribute.

import type { Matrix4 } from 'three';
import type { Ray3 } from '../types';

/** The triangles of one drawn object: positions (object space, three.js), the triangle index and its blocks. */
export interface PickMesh {
  readonly positions: ArrayLike<number>;
  readonly index: ArrayLike<number>;
  /** Index ranges of the material blocks (`start`, `count`) and whether the block is two-sided. */
  readonly blocks: readonly {
    readonly start: number;
    readonly count: number;
    readonly twoSided: boolean;
  }[];
  /** Two joints per vertex (global `.chr` bone indices); `null` for rigid meshes. */
  readonly joints: ArrayLike<number> | null;
}

const EPS = 1e-9;

/**
 * Möller-Trumbore for one triangle: the distance `t >= 0` along the (not necessarily unit) direction, or `null`.
 * `front` / `back` select which side counts.
 */
function hitTriangle(
  ox: number,
  oy: number,
  oz: number,
  dx: number,
  dy: number,
  dz: number,
  ax: number,
  ay: number,
  az: number,
  bx: number,
  by: number,
  bz: number,
  cx: number,
  cy: number,
  cz: number,
  twoSided: boolean,
): number | null {
  const e1x = bx - ax;
  const e1y = by - ay;
  const e1z = bz - az;
  const e2x = cx - ax;
  const e2y = cy - ay;
  const e2z = cz - az;
  // p = d x e2
  const px = dy * e2z - dz * e2y;
  const py = dz * e2x - dx * e2z;
  const pz = dx * e2y - dy * e2x;
  const det = e1x * px + e1y * py + e1z * pz;
  // det > 0: the ray hits the counter-clockwise (front) side
  if (twoSided ? Math.abs(det) < EPS : det < EPS) return null;
  const inv = 1 / det;
  const tx = ox - ax;
  const ty = oy - ay;
  const tz = oz - az;
  const u = (tx * px + ty * py + tz * pz) * inv;
  if (u < 0 || u > 1) return null;
  // q = t x e1
  const qx = ty * e1z - tz * e1y;
  const qy = tz * e1x - tx * e1z;
  const qz = tx * e1y - ty * e1x;
  const v = (dx * qx + dy * qy + dz * qz) * inv;
  if (v < 0 || u + v > 1) return null;
  const t = (e2x * qx + e2y * qy + e2z * qz) * inv;
  return t >= 0 ? t : null;
}

/** Transforms vertex `v` of `positions` by `m` into `out`. */
function transformVertex(
  positions: ArrayLike<number>,
  v: number,
  m: Matrix4,
  out: [number, number, number],
): void {
  const e = m.elements;
  const x = positions[v * 3] ?? 0;
  const y = positions[v * 3 + 1] ?? 0;
  const z = positions[v * 3 + 2] ?? 0;
  out[0] = e[0] * x + e[4] * y + e[8] * z + e[12];
  out[1] = e[1] * x + e[5] * y + e[9] * z + e[13];
  out[2] = e[2] * x + e[6] * y + e[10] * z + e[14];
}

/**
 * The nearest hit of `ray` (three.js space) with a mesh. `matrixOf(joint)` is the matrix that takes a vertex bound to that
 * joint into world space (`world . palette[joint]`); `null` skips the vertex' triangles (an unknown bone). Rigid meshes pass a
 * constant matrix and no joints.
 */
export function rayVsMesh(
  ray: Ray3,
  mesh: PickMesh,
  matrixOf: (joint: number) => Matrix4 | null,
): number | null {
  let best: number | null = null;
  const [ox, oy, oz] = ray.origin;
  const [dx, dy, dz] = ray.dir;
  const a: [number, number, number] = [0, 0, 0];
  const b: [number, number, number] = [0, 0, 0];
  const c: [number, number, number] = [0, 0, 0];
  const cache = new Map<number, Matrix4 | null>();
  const mat = (joint: number): Matrix4 | null => {
    if (cache.has(joint)) return cache.get(joint) ?? null;
    const m = matrixOf(joint);
    cache.set(joint, m);
    return m;
  };
  const jointOf = (v: number): number => (mesh.joints ? (mesh.joints[v * 2] ?? 0) : 0);
  for (const block of mesh.blocks) {
    for (let i = block.start; i + 2 < block.start + block.count; i += 3) {
      const i0 = mesh.index[i] ?? 0;
      const i1 = mesh.index[i + 1] ?? 0;
      const i2 = mesh.index[i + 2] ?? 0;
      // the primary bone of the triangle: its first vertex' joint (the three are the same for a limb in practice)
      const m0 = mat(jointOf(i0));
      const m1 = mat(jointOf(i1));
      const m2 = mat(jointOf(i2));
      if (!m0 || !m1 || !m2) continue;
      transformVertex(mesh.positions, i0, m0, a);
      transformVertex(mesh.positions, i1, m1, b);
      transformVertex(mesh.positions, i2, m2, c);
      const t = hitTriangle(
        ox,
        oy,
        oz,
        dx,
        dy,
        dz,
        a[0],
        a[1],
        a[2],
        b[0],
        b[1],
        b[2],
        c[0],
        c[1],
        c[2],
        block.twoSided,
      );
      if (t !== null && (best === null || t < best)) best = t;
    }
  }
  return best;
}
