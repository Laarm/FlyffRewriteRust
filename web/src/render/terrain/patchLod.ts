// Patch level of detail and the land index buffer (Plan/41 section 9.3; lod.cpp:46-221, 1122-1170,
// World3D.cpp:385-442, 2178-2253; ref/19 section 16.4).
//
// Every frame each patch (16 x 16 per land, 8 x 8 cells) gets a level from the 3-D distance between the camera and
// the centre of the patch's bounding box: `> R3` level 3, `> R2` level 2, `> R1` level 1, else 0, with the ranges of
// the terrain view option (`RenderOptions.terrainView`, times MPU). Level `l` uses the vertex step `s = 2^l`
// (128, 32, 8, 2 triangles per patch). The surface keeps the TRUE grid heights (no geomorphing); cracks between
// patches of different levels are closed by stitching:
//
//   * a side whose neighbour is COARSER (step `s' > s`, also across land borders; a missing neighbour counts as the
//     same level) only uses the border vertices at multiples of `s'`: every other border vertex of this patch is
//     moved ALONG the border to the nearer coarse vertex, which turns the border cell row into fans from the coarse
//     vertices to the fine inner row (triangles that collapse are dropped);
//   * the finer side always adapts to the coarser one, so both patches share exactly the coarse border polyline and
//     the mesh is watertight for EVERY combination of neighbour levels (tested exhaustively).
//
// The cell triangulation is the checkerboard of `GetLandTri` (ref/19 section 15.3): the cell at step `s` with the
// global cell coordinate `(cx, cz)` uses the diagonal (0,0)-(1,1) when `(cx + cz)` is even, else (1,0)-(0,1); the
// level-0 patches therefore reproduce section 9.2 exactly. The original's hand-written coarse index tables use a
// different pattern for levels 1-3; the plan (section 9.3) specifies the checkerboard at every level.
//
// Index order: the triangles are listed in the D3D order (clockwise in the LH xz plane) and emitted as `(a, c, b)`
// because the three.js space mirrors z (front faces are counter-clockwise there).

import { PATCH_SIZE, PATCHES_PER_SIDE, PATCH_COUNT, VERTS } from './landData';

export type PatchLevel = 0 | 1 | 2 | 3;

/** Vertex step of a level. */
export const STEP_OF_LEVEL: readonly [number, number, number, number] = [1, 2, 4, 8];

/** Maximum triangles of a patch (level 0) and the index capacity of a land. */
export const MAX_TRIANGLES_PER_PATCH = 128;
export const LAND_INDEX_CAPACITY = PATCH_COUNT * MAX_TRIANGLES_PER_PATCH * 3;

/** `RenderOptions.terrainView`: LOD ranges `[R1, R2, R3]` in cells (times MPU, ref/21 section 12). */
export const LOD_RANGES_CELLS: Readonly<Record<0 | 1 | 2, readonly [number, number, number]>> = {
  0: [64, 128, 256],
  1: [32, 64, 128],
  2: [16, 32, 64],
};

/** The ranges in world units. Indoor worlds force view option 0 (HwOption note, ref/21 section 12). */
export function levelRanges(
  view: 0 | 1 | 2,
  mpu: number,
  indoor = false,
): readonly [number, number, number] {
  const r = LOD_RANGES_CELLS[indoor ? 0 : view];
  return [r[0] * mpu, r[1] * mpu, r[2] * mpu];
}

/** `CalculateLevel`: `> R3` -> 3, `> R2` -> 2, `> R1` -> 1, else 0. */
export function levelForDistance(
  distance: number,
  ranges: readonly [number, number, number],
): PatchLevel {
  if (distance > ranges[2]) return 3;
  if (distance > ranges[1]) return 2;
  if (distance > ranges[0]) return 1;
  return 0;
}

/** The coarse spacing of the border vertices of each side of a patch (`>= ` the patch's own step). */
export interface SideSteps {
  readonly west: number;
  readonly east: number;
  readonly south: number;
  readonly north: number;
}

type Off = readonly [number, number];
const EVEN: readonly (readonly [Off, Off, Off])[] = [
  [
    [0, 0],
    [1, 1],
    [1, 0],
  ],
  [
    [0, 0],
    [0, 1],
    [1, 1],
  ],
];
const ODD: readonly (readonly [Off, Off, Off])[] = [
  [
    [0, 0],
    [0, 1],
    [1, 0],
  ],
  [
    [1, 0],
    [0, 1],
    [1, 1],
  ],
];

/** Moves a border coordinate to the nearer coarse vertex (ties go to the higher one) when `coarse > own`. */
function parentOnBorder(c: number, own: number, coarse: number): number {
  if (coarse <= own) return c;
  const k = Math.floor(c / coarse);
  const rem = c - k * coarse;
  if (rem === 0) return c;
  return rem * 2 < coarse ? k * coarse : (k + 1) * coarse;
}

/**
 * The triangles of a patch of `level` with the given stitched sides as land-local vertex offsets
 * (`vz * 129 + vx` relative to the patch origin), three per triangle, already in the three.js winding.
 * `oddPatch` is `(px + pz) & 1` and only matters at level 3 (one cell per patch).
 */
export function patchOffsets(level: PatchLevel, sides: SideSteps, oddPatch: boolean): Uint16Array {
  const s = STEP_OF_LEVEL[level];
  const n = PATCH_SIZE / s;
  const out: number[] = [];
  const map = (x: number, z: number): [number, number] => {
    let mx = x;
    let mz = z;
    if (z === 0) mx = parentOnBorder(x, s, sides.south);
    else if (z === PATCH_SIZE) mx = parentOnBorder(x, s, sides.north);
    if (x === 0) mz = parentOnBorder(z, s, sides.west);
    else if (x === PATCH_SIZE) mz = parentOnBorder(z, s, sides.east);
    return [mx, mz];
  };
  for (let cz = 0; cz < n; cz++) {
    for (let cx = 0; cx < n; cx++) {
      const odd = (cx + cz + (level === 3 && oddPatch ? 1 : 0)) & 1;
      for (const tri of odd ? ODD : EVEN) {
        const p = tri.map((o) => map((cx + o[0]) * s, (cz + o[1]) * s));
        const [a, b, c] = p;
        if (!a || !b || !c) continue;
        if ((a[0] === b[0] && a[1] === b[1]) || (a[0] === c[0] && a[1] === c[1])) continue;
        if (b[0] === c[0] && b[1] === c[1]) continue;
        // collinear after the collapse (zero area): drop
        const area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if (area === 0) continue;
        const at = (q: readonly [number, number]): number => q[1] * VERTS + q[0];
        // listed (a, b, c) clockwise in LH; three.js winding is (a, c, b)
        out.push(at(a), at(c), at(b));
      }
    }
  }
  return Uint16Array.from(out);
}

const cache = new Map<number, Uint16Array>();

/** Cached `patchOffsets`; the key packs level, the four side steps (1/2/4/8 -> 2 bits) and the level-3 parity. */
export function cachedPatchOffsets(
  level: PatchLevel,
  sides: SideSteps,
  oddPatch: boolean,
): Uint16Array {
  const code = (st: number): number => (st === 1 ? 0 : st === 2 ? 1 : st === 4 ? 2 : 3);
  const key =
    level |
    (code(sides.west) << 2) |
    (code(sides.east) << 4) |
    (code(sides.south) << 6) |
    (code(sides.north) << 8) |
    ((level === 3 && oddPatch ? 1 : 0) << 10);
  let v = cache.get(key);
  if (!v) {
    v = patchOffsets(level, sides, oddPatch);
    cache.set(key, v);
  }
  return v;
}

/** The level of the patch `(px, pz)` of the land being built, or of its neighbour across a border (`-1`/`16`). */
export type LevelAt = (px: number, pz: number) => PatchLevel | undefined;

/**
 * Writes the index buffer of a land (all 256 patches) into `out` (capacity `LAND_INDEX_CAPACITY`) and returns the
 * number of indices. `levelAt` resolves the levels of the patches of this land and, for `px`/`pz` of `-1` and `16`,
 * of the adjacent lands; `undefined` (land not resident) counts as the same level.
 */
export function buildLandIndices(levelAt: LevelAt, out: Uint16Array): number {
  let n = 0;
  for (let pz = 0; pz < PATCHES_PER_SIDE; pz++) {
    for (let px = 0; px < PATCHES_PER_SIDE; px++) {
      const level = levelAt(px, pz);
      if (level === undefined) continue;
      const own = STEP_OF_LEVEL[level];
      const step = (qx: number, qz: number): number => {
        const l = levelAt(qx, qz);
        return Math.max(own, l === undefined ? own : STEP_OF_LEVEL[l]);
      };
      const sides: SideSteps = {
        west: step(px - 1, pz),
        east: step(px + 1, pz),
        south: step(px, pz - 1),
        north: step(px, pz + 1),
      };
      const offsets = cachedPatchOffsets(level, sides, ((px + pz) & 1) === 1);
      const base = pz * PATCH_SIZE * VERTS + px * PATCH_SIZE;
      for (const o of offsets) out[n++] = base + o;
    }
  }
  return n;
}

/** The patch AABB centre distance of `LODLEVELRANGE` (LH world space). */
export function patchDistance(
  camera: readonly [number, number, number],
  landOriginX: number,
  landOriginZ: number,
  mpu: number,
  px: number,
  pz: number,
  minY: number,
  maxY: number,
): number {
  const span = PATCH_SIZE * mpu;
  const cx = landOriginX + px * span + span / 2;
  const cz = landOriginZ + pz * span + span / 2;
  const cy = (minY + maxY) / 2;
  return Math.hypot(camera[0] - cx, camera[1] - cy, camera[2] - cz);
}

/**
 * Per-land patch levels: `update` recomputes them from the camera and reports whether any changed. A patch with a
 * missing level (`levelAt` `undefined`) never happens inside a land; the neighbour lookup across borders is the
 * caller's (`Terrain`).
 */
export class PatchLevels {
  readonly levels = new Uint8Array(PATCH_COUNT);
  /** `true` after construction and after `invalidate()`: the first index build is due. */
  dirty = true;

  /**
   * @param cameraLH eye position in LH world space
   * @param landOrigin world position `(x, z)` of vertex (0, 0) of the land
   * @param patchBounds `[minY, maxY]` per patch (`landData.patchBounds`)
   * @returns `true` when at least one level changed
   */
  update(
    cameraLH: readonly [number, number, number],
    landOrigin: readonly [number, number],
    mpu: number,
    patchBounds: Float32Array,
    ranges: readonly [number, number, number],
  ): boolean {
    let changed = false;
    for (let pz = 0; pz < PATCHES_PER_SIDE; pz++) {
      for (let px = 0; px < PATCHES_PER_SIDE; px++) {
        const p = px + pz * PATCHES_PER_SIDE;
        const d = patchDistance(
          cameraLH,
          landOrigin[0],
          landOrigin[1],
          mpu,
          px,
          pz,
          patchBounds[p * 2] ?? 0,
          patchBounds[p * 2 + 1] ?? 0,
        );
        const l = levelForDistance(d, ranges);
        if (this.levels[p] !== l) {
          this.levels[p] = l;
          changed = true;
        }
      }
    }
    if (changed) this.dirty = true;
    return changed;
  }

  /** The level of patch `(px, pz)` (`undefined` outside this land). */
  at(px: number, pz: number): PatchLevel | undefined {
    if (px < 0 || pz < 0 || px >= PATCHES_PER_SIDE || pz >= PATCHES_PER_SIDE) return undefined;
    return (this.levels[px + pz * PATCHES_PER_SIDE] ?? 0) as PatchLevel;
  }

  invalidate(): void {
    this.dirty = true;
  }

  /** Counts of patches per level (probe, tests). */
  histogram(): [number, number, number, number] {
    const h: [number, number, number, number] = [0, 0, 0, 0];
    for (const l of this.levels) h[l as PatchLevel] += 1;
    return h;
  }
}
