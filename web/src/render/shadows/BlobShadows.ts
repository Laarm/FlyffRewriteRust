// Blob shadows (Plan/41 section 13.1, WP-REND-13; default shadow option 2; World3D.cpp:993-1089, ref/18 section 8.6, ref/21
// section 6.3.2 step 9): `Shadow.o3d` under every drawn, non-invisible mover, oriented to the triangle of the terrain it stands
// on, drawn after the opaque models with alpha blending, no fog and a depth bias against z-fighting on slopes.
//
//   skip when the ground is under water (`currentHeight < byWaterHeight`), when the mover is more than 10 above the ground
//   or below it (`pos.y - currentHeight`), or in TRANSPARENT_MODE;
//   world = R(q) * Translate(x, currentHeight, z) in D3D row-vector order with `q` the rotation that takes (0, -1, 0) to the
//   triangle normal `cross(t2 - t0, t1 - t0)` of `GetLandTri` (the identity on flat ground; pets use their smoothed screen
//   position); the shadow does not scale with the mover.
//
// All shadows of a frame are ONE instanced draw (the model is rigid; per-instance matrix like the static objects). The terrain
// triangle comes from the CPU height field, which is visual-only data (R15); `currentHeight` is the sim's own value (render
// view slot 29), the height field never feeds the simulation.

import {
  InstancedBufferGeometry,
  Matrix4,
  Mesh,
  Sphere,
  Vector3,
  type InstancedBufferAttribute,
  type Object3D,
  type Texture,
} from 'three';
import { Prio } from '../../workers/priority';
import { multiply, quaternionRotationAxis, rotationQuaternion, translation } from '../d3dx';
import {
  createInstancedAttribute,
  exceedsInstanceLimit,
  growInstanced,
  instanceLimit,
  releaseInstancedGeometry,
} from '../gl/instancing';
import { Geo, keyString, type MaterialKey } from '../gl/materials/key';
import { makeInstanced } from '../objects/instancedShader';
import type { MaterialFactory, ModelMaterial } from '../gl/materials/MaterialFactory';
import { modelTexturePath, textureCandidates } from '../gl/textureNames';
import type { TextureCache, TextureHandle } from '../gl/textures';
import { LAYER } from '../layers';
import type { ModelGeometry } from '../objects/ModelCache';
import { writeInstance } from '../objects/placement';
import { mirrorMatrix } from '../space';
import type { HeightField, LandTri, Vec3 } from '../terrain/heightField';
import type { ModelProvider } from '../movers/MoverVisual';
import type { MoverEntry } from '../movers/MoverBatcher';

/** The model of the blob shadow (`g_Shadow.LoadElement("Shadow.o3d")`, Neuz.cpp:1648). */
export const SHADOW_MODEL = 'model/shadow.o3d';
/** A mover more than this above its ground has no shadow (`> 10.0f`). */
export const MAX_HEIGHT_ABOVE_GROUND = 10;

export type ShadowSkip = 'water' | 'high' | 'below';

/**
 * The skip rules of the original loop, in its order: ground under water (`currentHeight < byWaterHeight`), more than 10 above
 * the ground, below the ground (the mover is inside the floor). `null` = the mover gets a shadow.
 */
export function shadowSkip(
  posY: number,
  currentHeight: number,
  waterHeight: number,
): ShadowSkip | null {
  if (currentHeight < waterHeight) return 'water';
  if (posY - currentHeight > MAX_HEIGHT_ABOVE_GROUND) return 'high';
  if (posY - currentHeight < 0) return 'below';
  return null;
}

/**
 * The rotation of the shadow quad as the original builds it: `n = normalize(cross(t2 - t0, t1 - t0))`, `q` = rotation about
 * `cross((0, -1, 0), n)` by `acos(dot((0, -1, 0), n))`. `normal` is the unit normal `LandTri.normal` of the triangle
 * (`cross(t1 - t0, t2 - t0)`, +Y up on flat ground), which is `-n`. Returns the D3D row-major matrix (identity on flat ground
 * and without a triangle).
 */
export function shadowRotationLH(normal: Vec3 | null): number[] {
  if (normal === null) return rotationQuaternion([0, 0, 0, 1]);
  // n = -normal; up = (0, -1, 0): dot(up, n) = normal.y, cross(up, n) = (normal.z, 0, -normal.x)
  const dot = Math.min(1, Math.max(-1, normal[1]));
  const axis: Vec3 = [normal[2], 0, -normal[0]];
  if (axis[0] === 0 && axis[2] === 0) return rotationQuaternion([0, 0, 0, 1]); // flat (or exactly upside down: D3DX gives a NaN-free zero axis)
  return rotationQuaternion(quaternionRotationAxis(axis, Math.acos(dot)));
}

/** The LH world matrix of the shadow of a mover at `(x, currentHeight, z)` on a triangle with `normal`: `R * Translate`. */
export function shadowMatrixLH(
  x: number,
  currentHeight: number,
  z: number,
  normal: Vec3 | null,
): number[] {
  return multiply(shadowRotationLH(normal), translation(x, currentHeight, z));
}

/** The same matrix in three.js space (the elements of a `Matrix4`). */
export function shadowMatrixThree(
  x: number,
  currentHeight: number,
  z: number,
  normal: Vec3 | null,
  out: Matrix4 = new Matrix4(),
): Matrix4 {
  return out.fromArray(mirrorMatrix(shadowMatrixLH(x, currentHeight, z, normal)));
}

/** The material key of the blob shadow: texture colour and alpha, SRCALPHA / INVSRCALPHA, no lighting, no fog. */
export const SHADOW_KEY: MaterialKey = {
  geo: Geo.Rigid,
  alphaTest: false,
  blend: 'alpha',
  twoSided: true,
  unlit: true,
  reflect: false,
  hair: false,
  fog: 'off',
  instanced: true,
  shadowRecv: false,
  nightOnly: false,
  queue: 'translucent',
};

export interface BlobShadowDeps {
  /** The shared model scene (the shadow meshes sit in `LAYER.shadow`). */
  readonly scene: Object3D;
  readonly materials: MaterialFactory;
  readonly textures: TextureCache;
  /** Loads `model/shadow.o3d`. */
  readonly models: ModelProvider;
  readonly log?: (message: string) => void;
}

interface Group {
  readonly mesh: Mesh;
  /** Replaced by a bigger one when the group grows (`growInstanced`: three.js caches the instance limit per geometry). */
  geometry: InstancedBufferGeometry;
  attrs: InstancedBufferAttribute[];
  capacity: number;
  readonly chain: Matrix4;
  material: ModelMaterial | null;
  handle: TextureHandle | null;
  ready: boolean;
}

const tmp = new Matrix4();
const out12 = new Float32Array(12);

export class BlobShadows {
  private groups: Group[] = [];
  private requested = false;
  private disposed = false;
  /** Shadows written by the last `update` (probe `render.shadows`). */
  count = 0;
  /** Movers skipped by the last `update` per rule. */
  readonly skipped: Record<ShadowSkip, number> = { water: 0, high: 0, below: 0 };
  /** Enabled by the render option `shadow == 2`. */
  enabled = true;

  constructor(private readonly d: BlobShadowDeps) {}

  /** `true` once the model and its texture are resident. */
  get ready(): boolean {
    return this.groups.length > 0 && this.groups.every((g) => g.ready);
  }

  private load(): void {
    if (this.requested) return;
    this.requested = true;
    void this.d.models.model(SHADOW_MODEL, Prio.Near).then((model) => {
      if (this.disposed) return;
      if (!model) {
        this.d.log?.(`blob shadow: ${SHADOW_MODEL} is missing, no shadows`);
        return;
      }
      this.build(model);
    });
  }

  private build(model: ModelGeometry): void {
    const objects = model.lods[0] ?? [];
    const chains: Matrix4[] = [];
    for (const obj of objects) {
      const local = obj.kind === 'rigid' ? obj.localTM : new Matrix4();
      const parent = obj.parent.kind === 'object' ? chains[obj.parent.index] : undefined;
      chains.push(parent ? parent.clone().multiply(local) : local.clone());
    }
    objects.forEach((obj, oi) => {
      for (const block of obj.blocks) {
        const g = new InstancedBufferGeometry();
        g.setAttribute('position', block.geometry.getAttribute('position'));
        g.setAttribute('normal', block.geometry.getAttribute('normal'));
        g.setAttribute('uv', block.geometry.getAttribute('uv'));
        const index = block.geometry.index;
        if (index) g.setIndex(index);
        g.setDrawRange(block.submesh.start, block.submesh.count);
        const capacity = 16;
        const attrs = [0, 1, 2].map(() => createInstancedAttribute(capacity, 4));
        attrs.forEach((a, i) => g.setAttribute(`aInst${String(i)}`, a));
        g.instanceCount = 0;
        g.boundingSphere = new Sphere(new Vector3(), Infinity);
        const group: Group = {
          mesh: new Mesh(g),
          geometry: g,
          attrs,
          capacity,
          chain: chains[oi] ?? new Matrix4(),
          material: null,
          handle: null,
          ready: false,
        };
        group.mesh.frustumCulled = false;
        group.mesh.matrixAutoUpdate = false;
        group.mesh.visible = false;
        group.mesh.name = `blob-shadow#${String(oi)}.${String(block.blockIndex)}`;
        group.mesh.layers.set(LAYER.shadow);
        group.mesh.renderOrder = 8;
        this.groups.push(group);
        this.d.scene.add(group.mesh);
        this.bindTexture(group, block.submesh.texture);
      }
    });
  }

  /** The block's own texture (`shadow.dds`, colour and alpha as they are). */
  private bindTexture(group: Group, name: string | null): void {
    const bind = (map: Texture | null): void => {
      const material = this.d.materials.create(SHADOW_KEY, { map, alpha: 1 });
      makeInstanced(material, SHADOW_KEY);
      // the shadow quad sits ON the ground: a depth bias against z-fighting (`D3DRS_DEPTHBIAS -0.0001`), no depth write
      material.depthWrite = false;
      material.polygonOffset = true;
      material.polygonOffsetFactor = -1;
      material.polygonOffsetUnits = -1;
      const old = group.material;
      group.material = material;
      group.mesh.material = material;
      old?.dispose();
      group.ready = true;
    };
    if (name === null || name === '') {
      bind(null);
      return;
    }
    const names = textureCandidates(name, {}).map((n) => modelTexturePath(n));
    void this.d.textures.acquireFirst(names, 'world', { prio: Prio.Near }).then((h) => {
      if (this.disposed) {
        h.release();
        return;
      }
      group.handle = h;
      bind(h.texture);
      h.onChange(() => {
        if (!this.disposed) bind(h.texture);
      });
    });
  }

  /**
   * One drawn frame: writes the instance matrices of the shadows of `movers` (the movers drawn in the last frame). `field` is
   * the CPU height field of the resident lands (`null` before the first land: shadows lie flat).
   */
  update(movers: readonly MoverEntry[], field: HeightField | null): void {
    this.count = 0;
    this.skipped.water = 0;
    this.skipped.high = 0;
    this.skipped.below = 0;
    if (this.disposed) return;
    if (!this.enabled) {
      for (const g of this.groups) {
        g.geometry.instanceCount = 0;
        g.mesh.visible = false;
      }
      return;
    }
    this.load();
    if (this.groups.length === 0) return;
    // the instance list of this frame
    const list: Matrix4[] = [];
    for (const e of movers) {
      if (!e.decorated || !e.visual) continue;
      const water = field?.waterAt(e.pos[0], e.pos[2])?.height ?? 0;
      const why = shadowSkip(e.pos[1], e.currentHeight, water);
      if (why !== null) {
        this.skipped[why]++;
        continue;
      }
      // pets stand on their smoothed screen position, everybody else on the logical one
      const x = e.pet ? e.drawPos[0] : e.pos[0];
      const z = e.pet ? e.drawPos[2] : e.pos[2];
      const tri: LandTri | null = field?.triangleAt(x, z) ?? null;
      list.push(shadowMatrixThree(x, e.currentHeight, z, tri?.normal ?? null));
    }
    this.count = list.length;
    for (const g of this.groups) {
      const n = g.ready ? list.length : 0;
      if (n > g.capacity) this.grow(g, n);
      const [a0, a1, a2] = g.attrs;
      if (!a0 || !a1 || !a2) continue;
      for (let i = 0; i < n; i++) {
        const m = list[i];
        if (!m) continue;
        tmp.multiplyMatrices(m, g.chain);
        writeInstance(tmp, out12, 0);
        const w = i * 4;
        (a0.array as Float32Array).set(out12.subarray(0, 4), w);
        (a1.array as Float32Array).set(out12.subarray(4, 8), w);
        (a2.array as Float32Array).set(out12.subarray(8, 12), w);
      }
      g.geometry.instanceCount = n;
      for (const a of g.attrs) {
        a.clearUpdateRanges();
        if (n > 0) a.addUpdateRange(0, n * 4);
        a.needsUpdate = n > 0;
      }
      g.mesh.visible = n > 0;
    }
  }

  /**
   * Doubles the capacity of a group until `need` instances fit. `update` writes all `need` instances right after, so nothing is
   * copied. The bigger attributes go into a NEW geometry (`growInstanced`): three.js caches the instance limit of a geometry at
   * its first draw and never refreshes it, and a capped group lost the shadows beyond its first capacity (#018 B1).
   */
  private grow(g: Group, need: number): void {
    let cap = g.capacity;
    while (cap < need) cap *= 2;
    const next = growInstanced(g.mesh, cap, false);
    g.geometry = next;
    g.attrs = [0, 1, 2].map(
      (i) => next.getAttribute(`aInst${String(i)}`) as InstancedBufferAttribute,
    );
    g.capacity = cap;
  }

  /** The materials in use (probe, tests). */
  materialKeys(): string[] {
    return this.groups.map((g) => (g.material ? keyString(g.material.userData.key) : ''));
  }

  /** The meshes (tests). */
  get meshes(): readonly Mesh[] {
    return this.groups.map((g) => g.mesh);
  }

  /**
   * The groups whose geometry asks for more instances than three.js will draw (#018 B1: the limit is cached at the first draw).
   * Always empty while every growth goes through `growInstanced`; the Playwright check and the tests read it.
   */
  instanceLimitViolations(): { name: string; count: number; limit: number }[] {
    const out: { name: string; count: number; limit: number }[] = [];
    for (const g of this.groups) {
      if (exceedsInstanceLimit(g.geometry)) {
        out.push({
          name: g.mesh.name,
          count: g.geometry.instanceCount,
          limit: instanceLimit(g.geometry),
        });
      }
    }
    return out;
  }

  /** Draw calls the shadows cost now (instanced groups with instances). */
  get drawCalls(): number {
    return this.groups.filter((g) => g.mesh.visible).length;
  }

  dispose(): void {
    this.disposed = true;
    for (const g of this.groups) {
      g.mesh.removeFromParent();
      // position, normal, uv and the index belong to the model cache's shadow model: detached before `dispose` (#018 B1)
      releaseInstancedGeometry(g.geometry);
      g.material?.dispose();
      g.handle?.release();
    }
    this.groups = [];
  }
}
