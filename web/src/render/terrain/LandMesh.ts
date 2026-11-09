// The mesh of one land (Plan/41 section 9.2, WP-REND-08): 129 x 129 SHARED vertices (the original duplicated 9 x 9 per
// patch; the surface is identical), one dynamic index buffer rebuilt when a patch level changes (`patchLod.ts`).
//
// Vertex positions are LAND-LOCAL in three.js space: `(vx * mpu, h, -(vz * mpu))` (z mirrored, `render/space.ts`); the
// mesh's matrix is the translation to the land origin `(lx * 128 * mpu, 0, -(lz * 128 * mpu))`, so three.js composes
// the model-view matrix of a far land in doubles on the CPU (a land 15 000 units from the origin keeps its precision).
// The vertex shader recovers the cell coordinate from the position: `cell = (position.x, -position.z) / mpu`.

import { BufferAttribute, BufferGeometry, Box3, DynamicDrawUsage, Sphere, Vector3 } from 'three';
import type { LandData } from './landData';
import { MAP_SIZE, VERTS, VERTEX_COUNT } from './landData';
import { buildLandIndices, LAND_INDEX_CAPACITY, type LevelAt } from './patchLod';

export class LandMesh {
  readonly geometry = new BufferGeometry();
  /** `x y z` per vertex, land-local three.js space. */
  readonly positions: Float32Array;
  /** `x y z` per vertex, three.js space (see `normals.ts`). */
  readonly normals: Float32Array;
  private readonly indexData = new Uint16Array(LAND_INDEX_CAPACITY);
  private readonly indexAttr: BufferAttribute;
  private readonly positionAttr: BufferAttribute;
  private readonly normalAttr: BufferAttribute;
  /** Indices currently valid (`setDrawRange(0, indexCount)`). */
  indexCount = 0;
  /** Land-local bounds (three.js space) for frustum culling: the geometry's bounding box is exact. */
  readonly box = new Box3();
  readonly sphere = new Sphere();
  /** The land origin in LH world space. */
  readonly originX: number;
  readonly originZ: number;
  readonly mpu: number;

  constructor(land: LandData) {
    this.mpu = land.mpu;
    this.originX = land.x * MAP_SIZE * land.mpu;
    this.originZ = land.z * MAP_SIZE * land.mpu;
    this.positions = new Float32Array(VERTEX_COUNT * 3);
    this.normals = new Float32Array(VERTEX_COUNT * 3);
    const mpu = land.mpu;
    for (let vz = 0; vz < VERTS; vz++) {
      for (let vx = 0; vx < VERTS; vx++) {
        const i = vz * VERTS + vx;
        this.positions[i * 3] = vx * mpu;
        this.positions[i * 3 + 1] = land.heights[i] ?? 0;
        this.positions[i * 3 + 2] = -(vz * mpu);
      }
    }
    this.positionAttr = new BufferAttribute(this.positions, 3);
    this.normalAttr = new BufferAttribute(this.normals, 3);
    this.indexAttr = new BufferAttribute(this.indexData, 1);
    this.indexAttr.setUsage(DynamicDrawUsage);
    this.geometry.setAttribute('position', this.positionAttr);
    this.geometry.setAttribute('normal', this.normalAttr);
    this.geometry.setIndex(this.indexAttr);
    this.geometry.setDrawRange(0, 0);
    this.box.set(
      new Vector3(0, land.bounds[0], -MAP_SIZE * mpu),
      new Vector3(MAP_SIZE * mpu, land.bounds[1], 0),
    );
    this.box.getBoundingSphere(this.sphere);
    this.geometry.boundingBox = this.box.clone();
    this.geometry.boundingSphere = this.sphere.clone();
  }

  /** Marks the normals for upload (after `computeNormals` / `recomputeBorderNormals`). */
  normalsChanged(): void {
    this.normalAttr.needsUpdate = true;
  }

  /**
   * Rebuilds the index buffer from the patch levels (`LevelAt` also resolves the neighbouring lands' levels at
   * `-1` / `16`). Uploads only the used range.
   */
  rebuildIndices(levelAt: LevelAt): number {
    const n = buildLandIndices(levelAt, this.indexData);
    this.indexCount = n;
    this.geometry.setDrawRange(0, n);
    this.indexAttr.clearUpdateRanges();
    this.indexAttr.addUpdateRange(0, n);
    this.indexAttr.needsUpdate = true;
    return n;
  }

  get triangleCount(): number {
    return this.indexCount / 3;
  }

  /** Estimated GPU bytes (positions, normals, the index buffer at its capacity). */
  get gpuBytes(): number {
    return this.positions.byteLength + this.normals.byteLength + this.indexData.byteLength;
  }

  /** The positions are kept (the heights belong to the pack); the GPU buffers go with the geometry. */
  dispose(): void {
    this.geometry.dispose();
  }
}
