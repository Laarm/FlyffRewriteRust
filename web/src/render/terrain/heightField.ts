// CPU copy of the loaded lands' heights for VISUAL queries only (Plan/41 section 9.6, R15): `heightAt` (bilinear,
// = `GetLandHeight`), `heightFast` (lower-left vertex, rain), `triangleAt` (= `GetLandTri`, ref/19 section 15.3),
// `waterAt`, plus the cross-land vertex lookup the normals need. The results position or tilt cosmetic geometry
// (blob shadows, rain splashes, cloud/water lookups, the underwater test, ground-item fallback); they NEVER feed
// `SimWorld`, protocol messages, stores or a gameplay decision (R15, lint: only `src/render/**` may import this file).
//
// Coordinates are LH world space: land `(lx, lz)` covers `x in [lx * 128 * mpu, (lx + 1) * 128 * mpu)`; vertex
// `(vx, vz)` of a land sits at `((lx * 128 + vx) * mpu, h[vz * 129 + vx], (lz * 128 + vz) * mpu)`. Edge vertices
// are shared: column 128 of land X is column 0 of land X + 1.

import { MAP_SIZE, PATCH_SIZE, PATCHES_PER_SIDE, VERTS, WTYPE } from './landData';

/** What the field keeps of a land (a view over the decoded pack: no copy). */
export interface FieldLand {
  readonly heights: Float32Array;
  readonly water: Uint8Array;
}

export type Vec3 = readonly [number, number, number];

/** The triangle of `GetLandTri`: three LH world positions and the unit face normal (+Y up on flat ground). */
export interface LandTri {
  readonly a: Vec3;
  readonly b: Vec3;
  readonly c: Vec3;
  readonly normal: Vec3;
}

export interface WaterAt {
  /** `byWaterHeight`. */
  readonly height: number;
  /** `WTYPE_*`: 0 none, 1 cloud, 2 water (the water bits only: `tex & 3`; `cloud` is the unmasked compare). */
  readonly type: number;
  /** `tex >> 2`: the water animation set. */
  readonly set: number;
}

/**
 * The triangle of cell `(gx, gz)` that contains the fractional position `(glx, glz)` (0..1 each), as vertex
 * offsets `(dx, dz)` in the order of `GetLandTri` (World.cpp:1251-1353):
 *
 *     (gx + gz) even:  glx > glz  -> (0,0) (1,1) (1,0)   else (0,0) (0,1) (1,1)
 *     (gx + gz) odd:   glx + glz < 1 -> (0,0) (0,1) (1,0)   else (1,0) (0,1) (1,1)
 *
 * The rendering index buffers use the same pattern (World3D.cpp:2150-2170), so a triangle that contains a point on
 * the surface is the triangle the mesh draws there.
 */
export function cellTriangleOffsets(
  gx: number,
  gz: number,
  glx: number,
  glz: number,
): readonly [readonly [number, number], readonly [number, number], readonly [number, number]] {
  if (((gx + gz) & 1) === 0) {
    return glx > glz
      ? [
          [0, 0],
          [1, 1],
          [1, 0],
        ]
      : [
          [0, 0],
          [0, 1],
          [1, 1],
        ];
  }
  return glx + glz < 1
    ? [
        [0, 0],
        [0, 1],
        [1, 0],
      ]
    : [
        [1, 0],
        [0, 1],
        [1, 1],
      ];
}

function normalOf(a: Vec3, b: Vec3, c: Vec3): Vec3 {
  const ux = b[0] - a[0];
  const uy = b[1] - a[1];
  const uz = b[2] - a[2];
  const vx = c[0] - a[0];
  const vy = c[1] - a[1];
  const vz = c[2] - a[2];
  const nx = uy * vz - uz * vy;
  const ny = uz * vx - ux * vz;
  const nz = ux * vy - uy * vx;
  const len = Math.hypot(nx, ny, nz);
  return len > 0 ? [nx / len, ny / len, nz / len] : [0, 1, 0];
}

export class HeightField {
  private readonly lands = new Map<number, FieldLand>();

  constructor(
    readonly mpu: number,
    readonly landsW: number,
    readonly landsH: number,
  ) {}

  private key(lx: number, lz: number): number {
    return lx + lz * this.landsW;
  }

  inWorld(lx: number, lz: number): boolean {
    return lx >= 0 && lz >= 0 && lx < this.landsW && lz < this.landsH;
  }

  /** Registers (or with `null` removes) the heights of land `(lx, lz)`. */
  setLand(lx: number, lz: number, land: FieldLand | null): void {
    if (!this.inWorld(lx, lz)) return;
    if (land) this.lands.set(this.key(lx, lz), land);
    else this.lands.delete(this.key(lx, lz));
  }

  hasLand(lx: number, lz: number): boolean {
    return this.inWorld(lx, lz) && this.lands.has(this.key(lx, lz));
  }

  get landCount(): number {
    return this.lands.size;
  }

  /**
   * The height of vertex `(vx, vz)` of land `(lx, lz)`; `vx`/`vz` may reach one step beyond `0..128`, which resolves
   * into the 4-neighbour land (`-1` -> its vertex 127, `129` -> its vertex 1). `undefined` when that land is not
   * resident or outside the world.
   */
  neighbourHeight(lx: number, lz: number, vx: number, vz: number): number | undefined {
    let x = lx;
    let z = lz;
    let ix = vx;
    let iz = vz;
    if (ix < 0) {
      x--;
      ix += MAP_SIZE;
    } else if (ix > MAP_SIZE) {
      x++;
      ix -= MAP_SIZE;
    }
    if (iz < 0) {
      z--;
      iz += MAP_SIZE;
    } else if (iz > MAP_SIZE) {
      z++;
      iz -= MAP_SIZE;
    }
    if (ix < 0 || iz < 0 || ix > MAP_SIZE || iz > MAP_SIZE) return undefined;
    return this.lands.get(this.key(x, z))?.heights[iz * VERTS + ix];
  }

  /** The height of the GLOBAL vertex `(gx, gz)` (cells from the world origin), `undefined` outside the loaded lands. */
  vertexHeight(gx: number, gz: number): number | undefined {
    // vertex 128 of a land is vertex 0 of the next one: prefer the land that starts at the vertex, except at the edge
    let lx = Math.floor(gx / MAP_SIZE);
    let lz = Math.floor(gz / MAP_SIZE);
    let ix = gx - lx * MAP_SIZE;
    let iz = gz - lz * MAP_SIZE;
    if (!this.inWorld(lx, lz) || !this.lands.has(this.key(lx, lz))) {
      // a missing land owning the vertex: the previous land has it as its column/row 128
      if (ix === 0 && lx > 0) {
        lx--;
        ix = MAP_SIZE;
      }
      if (iz === 0 && lz > 0) {
        lz--;
        iz = MAP_SIZE;
      }
    }
    return this.lands.get(this.key(lx, lz))?.heights[iz * VERTS + ix];
  }

  /** The land that contains world position `(x, z)` and the cell coordinates inside it. */
  private locate(
    x: number,
    z: number,
  ): { land: FieldLand; fx: number; fz: number; lx: number; lz: number } | null {
    if (!(x >= 0 && z >= 0)) return null;
    const edge = MAP_SIZE * this.mpu;
    const lx = Math.floor(x / edge);
    const lz = Math.floor(z / edge);
    if (!this.inWorld(lx, lz)) return null;
    const land = this.lands.get(this.key(lx, lz));
    if (!land) return null;
    return { land, fx: x / this.mpu - lx * MAP_SIZE, fz: z / this.mpu - lz * MAP_SIZE, lx, lz };
  }

  /** `GetLandHeight`: the bilinear height at `(x, z)`, `0` outside the world or on a land that is not resident. */
  heightAt(x: number, z: number): number {
    const loc = this.locate(x, z);
    if (!loc) return 0;
    const px = Math.min(MAP_SIZE - 1, Math.floor(loc.fx));
    const pz = Math.min(MAP_SIZE - 1, Math.floor(loc.fz));
    const dx = loc.fx - px;
    const dz = loc.fz - pz;
    const h = loc.land.heights;
    const y1 = h[pz * VERTS + px] ?? 0;
    const y2 = h[pz * VERTS + px + 1] ?? 0;
    const y3 = h[(pz + 1) * VERTS + px] ?? 0;
    const y4 = h[(pz + 1) * VERTS + px + 1] ?? 0;
    return y1 * (1 - dx) * (1 - dz) + y2 * dx * (1 - dz) + y3 * (1 - dx) * dz + y4 * dx * dz;
  }

  /** `GetLandHeight_Fast`: the lower-left vertex of the cell (rain, splashes). */
  heightFast(x: number, z: number): number {
    const loc = this.locate(x, z);
    if (!loc) return 0;
    const px = Math.min(MAP_SIZE, Math.floor(loc.fx));
    const pz = Math.min(MAP_SIZE, Math.floor(loc.fz));
    return loc.land.heights[pz * VERTS + px] ?? 0;
  }

  /** `GetLandTri`: the triangle under `(x, z)` (LH world positions) or `null` when the land is not resident. */
  triangleAt(x: number, z: number): LandTri | null {
    const loc = this.locate(x, z);
    if (!loc) return null;
    const cx = Math.min(MAP_SIZE - 1, Math.floor(loc.fx));
    const cz = Math.min(MAP_SIZE - 1, Math.floor(loc.fz));
    const glx = loc.fx - cx;
    const glz = loc.fz - cz;
    const off = cellTriangleOffsets(loc.lx * MAP_SIZE + cx, loc.lz * MAP_SIZE + cz, glx, glz);
    const base = this.mpu;
    const v = (o: readonly [number, number]): Vec3 => [
      (loc.lx * MAP_SIZE + cx + o[0]) * base,
      loc.land.heights[(cz + o[1]) * VERTS + cx + o[0]] ?? 0,
      (loc.lz * MAP_SIZE + cz + o[1]) * base,
    ];
    const a = v(off[0]);
    const b = v(off[1]);
    const c = v(off[2]);
    return { a, b, c, normal: normalOf(a, b, c) };
  }

  /** The water/cloud record of the patch under `(x, z)`, `null` when the land is not resident. */
  waterAt(x: number, z: number): WaterAt | null {
    const loc = this.locate(x, z);
    if (!loc) return null;
    const px = Math.min(PATCHES_PER_SIDE - 1, Math.floor(loc.fx / PATCH_SIZE));
    const pz = Math.min(PATCHES_PER_SIDE - 1, Math.floor(loc.fz / PATCH_SIZE));
    const p = px + pz * PATCHES_PER_SIDE;
    const tex = loc.land.water[p * 2 + 1] ?? 0;
    return {
      height: loc.land.water[p * 2] ?? 0,
      type: tex === WTYPE.cloud ? WTYPE.cloud : tex & 3,
      set: tex >> 2,
    };
  }

  clear(): void {
    this.lands.clear();
  }
}
