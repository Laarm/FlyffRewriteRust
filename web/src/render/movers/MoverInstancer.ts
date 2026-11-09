// Draw-call batching of the world's movers (Plan/41 section 12.7 `MoverBatcher` batching, WP-REND-12; spike T4): movers that show
// the SAME skinned geometry with the same material state and the same textures are ONE instanced draw. Monsters of one type
// collapse to 1-3 draws, players share the default bodies and common armour sets.
//
// It works on top of the per-mover visuals, which stay the single source of what a mover looks like (models, texture rules, eye
// blink, hair colour, fades): after `MoverVisual.update` the instancer looks at the meshes the visual draws and, for every
// SKINNED, opaque, palette-bound one, delegates the draw to a batch:
//
//   batch key = geometry (the block of a model, or one material state of a merged body) + material key + constant alpha + the
//               textures (`uMap` or the 8 merged slots; the eye-blink texture swap moves a mover to another batch while its
//               eyes are closed) + the hair-lighting flag
//   per instance = the world matrix as the three rows of the render view, the first palette matrix, the hair colour
//
// The mover's own mesh stays in the scene, moved to the hidden layer while delegated (its materials and uniforms are what the
// highlight twin and the next frame read); a mesh that is no longer delegated returns to the layer of its pass. Rigid parts
// (weapons, shields), blended blocks and movers with a fade (model blend < 255) are not batched.

import {
  InstancedBufferGeometry,
  Mesh,
  Sphere,
  Vector3,
  type BufferGeometry,
  type InstancedBufferAttribute,
  type Matrix4,
  type Object3D,
  type Texture,
} from 'three';
import {
  createInstancedAttribute,
  exceedsInstanceLimit,
  growInstanced,
  instanceLimit,
  releaseInstancedGeometry,
} from '../gl/instancing';
import { Geo, keyString, type MaterialKey } from '../gl/materials/key';
import type { MaterialFactory, ModelMaterial } from '../gl/materials/MaterialFactory';
import { LAYER } from '../layers';
import { MERGE_SLOTS } from './merge';
import type { MoverVisual } from './MoverVisual';

export interface InstancerDeps {
  /** The shared model scene (the batch meshes sit next to the other model meshes). */
  readonly scene: Object3D;
  readonly materials: MaterialFactory;
  /** The palette texture uniform of the pose source (shared by every batch). */
  readonly palette: { value: Texture | null };
  /** Frames a batch without instances lives on before its buffers are freed (default 180). */
  readonly idleFrames?: number;
}

/** Counters of the last frame (probe `render.movers.instancing`). */
export interface InstancerStats {
  /** Batches that draw this frame. */
  batches: number;
  /** Instances over all batches (movers x meshes delegated). */
  instances: number;
  /** Meshes that were delegated this frame (the draws they would have cost on their own). */
  delegated: number;
  /** Batches alive (drawn or idle). */
  alive: number;
}

interface Batch {
  readonly key: string;
  /** Replaced by a bigger one when the batch grows (`growInstanced`: three.js caches the instance limit per geometry). */
  geometry: InstancedBufferGeometry;
  readonly mesh: Mesh;
  readonly material: ModelMaterial;
  readonly hair: boolean;
  capacity: number;
  rows: InstancedBufferAttribute[];
  base: InstancedBufferAttribute;
  hairColor: InstancedBufferAttribute | null;
  count: number;
  idle: number;
  pass: number;
}

/** What is remembered of a delegated mesh between frames (the batch it belongs to is looked up only when its inputs changed). */
interface MeshRecord {
  batch: Batch;
  material: ModelMaterial;
  alpha: number;
  maps: (Texture | null)[];
}

/** The textures a material samples: `uMap`, or the merged slots `uMap0 .. uMap7`. */
function mapsOf(material: ModelMaterial, out: (Texture | null)[]): number {
  const u = material.uniforms;
  const single = u['uMap'];
  if (single) {
    out[0] = single.value as Texture | null;
    return 1;
  }
  let n = 0;
  for (let i = 0; i < MERGE_SLOTS; i++) {
    const slot = u[`uMap${String(i)}`];
    if (!slot) break;
    out[n++] = slot.value as Texture | null;
  }
  return n;
}

export class MoverInstancer {
  /** Batching can be switched off (`?instancing=0` on the dev pages): every mesh then draws on its own. */
  enabled = true;
  readonly stats: InstancerStats = { batches: 0, instances: 0, delegated: 0, alive: 0 };
  private readonly batches = new Map<string, Batch>();
  private readonly meshes = new WeakMap<Mesh, MeshRecord>();
  /** Meshes delegated this frame / last frame (to give the others their layer back). */
  private nowSet = new Set<Mesh>();
  private prevSet = new Set<Mesh>();
  private pass = 0;
  /** Meshes the last `submit` found drawn (`Infinity` when it declined to batch): the caller refreshes the rest. */
  lastTotal = 0;
  private readonly scratch: (Texture | null)[] = new Array<Texture | null>(MERGE_SLOTS).fill(null);
  private readonly idleFrames: number;

  constructor(private readonly d: InstancerDeps) {
    this.idleFrames = d.idleFrames ?? 180;
  }

  beginFrame(): void {
    this.pass++;
    const t = this.prevSet;
    this.prevSet = this.nowSet;
    this.nowSet = t;
    this.nowSet.clear();
    for (const b of this.batches.values()) b.count = 0;
  }

  /**
   * Delegates the batchable meshes of a drawn mover to batches. `world` is the draw matrix of the mover (three.js space), `base`
   * its first palette matrix (the hair colour is read from the mover's hair material). Returns the number of meshes delegated.
   */
  submit(visual: MoverVisual, world: Matrix4, base: number): number {
    if (!this.enabled || visual.blend < 255) {
      this.lastTotal = Number.POSITIVE_INFINITY;
      return 0;
    }
    let n = 0;
    let total = 0;
    visual.forEachDrawnMesh((mesh) => {
      total++;
      const material = mesh.material;
      if (Array.isArray(material)) return;
      const m = material as ModelMaterial;
      const key = m.userData.key as MaterialKey | undefined;
      if (key?.geo !== Geo.Skinned || key.queue !== 'opaque') return;
      if (!m.uniforms['uPaletteBase']) return; // not bound to a mover palette
      const batch = this.batchFor(mesh, m, key);
      if (!batch) return;
      this.push(batch, world, base, m);
      this.nowSet.add(mesh);
      mesh.layers.set(LAYER.hidden); // the batch draws it now (set every frame: a material swap of the visual resets the layer)
      n++;
    });
    this.lastTotal = total;
    return n;
  }

  /** Writes the buffers, shows the batches that have instances, gives the meshes that are no longer delegated their layer back. */
  endFrame(): void {
    for (const mesh of this.prevSet) {
      if (this.nowSet.has(mesh)) continue;
      const k = (mesh.material as ModelMaterial).userData.key as MaterialKey | undefined;
      if (k) mesh.layers.set(LAYER[k.queue]);
    }
    let batches = 0;
    let instances = 0;
    for (const [key, b] of [...this.batches]) {
      if (b.count > 0) {
        b.idle = 0;
        batches++;
        instances += b.count;
        b.geometry.instanceCount = b.count;
        for (const a of [...b.rows, b.base, ...(b.hairColor ? [b.hairColor] : [])]) {
          a.clearUpdateRanges();
          a.addUpdateRange(0, b.count * a.itemSize);
          a.needsUpdate = true;
        }
        b.mesh.visible = true;
      } else {
        b.mesh.visible = false;
        b.geometry.instanceCount = 0;
        if (++b.idle > this.idleFrames) {
          this.disposeBatch(b);
          this.batches.delete(key);
        }
      }
    }
    this.stats.batches = batches;
    this.stats.instances = instances;
    this.stats.delegated = instances;
    this.stats.alive = this.batches.size;
  }

  private batchFor(mesh: Mesh, m: ModelMaterial, key: MaterialKey): Batch | null {
    const alpha = (m.uniforms['uAlpha']?.value as number | undefined) ?? 1;
    const n = mapsOf(m, this.scratch);
    const rec = this.meshes.get(mesh);
    if (rec?.material === m && rec.alpha === alpha && rec.batch.pass !== -1) {
      let same = rec.maps.length === n;
      for (let i = 0; same && i < n; i++) same = rec.maps[i] === this.scratch[i];
      if (same && this.batches.get(rec.batch.key) === rec.batch) return rec.batch;
    }
    const ids: string[] = [];
    for (let i = 0; i < n; i++) ids.push(String(this.scratch[i]?.id ?? 'w'));
    const k = `${mesh.geometry.uuid}|${keyString(key)}|${String(Math.round(alpha * 255))}|${ids.join(',')}|${
      key.hair ? String(m.uniforms['uHairLit']?.value) : ''
    }`;
    let batch = this.batches.get(k);
    if (!batch) {
      batch = this.createBatch(k, mesh.geometry, m, key);
      this.batches.set(k, batch);
    }
    this.meshes.set(mesh, { batch, material: m, alpha, maps: this.scratch.slice(0, n) });
    return batch;
  }

  private createBatch(
    key: string,
    source: BufferGeometry,
    m: ModelMaterial,
    mk: MaterialKey,
  ): Batch {
    const g = new InstancedBufferGeometry();
    for (const name of Object.keys(source.attributes)) {
      const a = source.getAttribute(name);
      g.setAttribute(name, a);
    }
    if (source.index) g.setIndex(source.index);
    g.setDrawRange(source.drawRange.start, source.drawRange.count);
    g.boundingSphere = new Sphere(new Vector3(), Infinity);
    const capacity = 16;
    const make = (size: number): InstancedBufferAttribute =>
      createInstancedAttribute(capacity, size);
    const rows = [0, 1, 2].map(() => make(4));
    rows.forEach((a, i) => g.setAttribute(`aInst${String(i)}`, a));
    const base = make(1);
    g.setAttribute('aPalBase', base);
    const hair = mk.hair;
    const hairColor = hair ? make(3) : null;
    if (hairColor) g.setAttribute('aHairColor', hairColor);
    g.instanceCount = 0;
    // the same textures as the mover's own material (by value at creation: a texture swap makes another batch)
    const slots: { value: Texture }[] = [];
    for (let i = 0; i < MERGE_SLOTS; i++) {
      const u = m.uniforms[`uMap${String(i)}`];
      if (!u) break;
      slots.push({ value: u.value as Texture });
    }
    const white = m.uniforms['uMap'];
    const hairBinding = hair
      ? {
          color: { value: new Vector3(1, 1, 1) },
          lit: m.uniforms['uHairLit'] as { value: number },
        }
      : undefined;
    const material = this.d.materials.create(mk, {
      alpha: (m.uniforms['uAlpha']?.value as number | undefined) ?? 1,
      instancedPalette: this.d.palette,
      name: `batch:${keyString(mk)}`,
      ...(white ? { map: white.value as Texture | null } : {}),
      ...(slots.length > 0 ? { slots } : {}),
      ...(hairBinding ? { hair: hairBinding } : {}),
    });
    const mesh = new Mesh(g, material);
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    mesh.visible = false;
    mesh.layers.set(LAYER[mk.queue]);
    mesh.name = `movers:${key.slice(0, 40)}`;
    this.d.scene.add(mesh);
    return {
      key,
      geometry: g,
      mesh,
      material,
      hair,
      capacity,
      rows,
      base,
      hairColor,
      count: 0,
      idle: 0,
      pass: this.pass,
    };
  }

  private push(b: Batch, world: Matrix4, base: number, m: ModelMaterial): void {
    if (b.count >= b.capacity) this.grow(b, b.count + 1);
    const i = b.count++;
    const e = world.elements;
    // the three rows of the 3x4 matrix, translation in w (the render view's layout)
    const r0 = b.rows[0]?.array as Float32Array;
    const r1 = b.rows[1]?.array as Float32Array;
    const r2 = b.rows[2]?.array as Float32Array;
    const o = i * 4;
    r0[o] = e[0];
    r0[o + 1] = e[4];
    r0[o + 2] = e[8];
    r0[o + 3] = e[12];
    r1[o] = e[1];
    r1[o + 1] = e[5];
    r1[o + 2] = e[9];
    r1[o + 3] = e[13];
    r2[o] = e[2];
    r2[o + 1] = e[6];
    r2[o + 2] = e[10];
    r2[o + 3] = e[14];
    (b.base.array as Float32Array)[i] = base;
    if (b.hairColor) {
      const c = m.uniforms['uHairColor']?.value as Vector3 | undefined;
      const h = b.hairColor.array as Float32Array;
      h[i * 3] = c?.x ?? 1;
      h[i * 3 + 1] = c?.y ?? 1;
      h[i * 3 + 2] = c?.z ?? 1;
    }
  }

  /**
   * Doubles the capacity of a batch until `need` instances fit, keeping what this frame wrote so far. three.js caches the
   * instance limit of a geometry at its FIRST draw and never refreshes it (`WebGLBindingStates.js:415-419`), so the bigger
   * attributes go into a NEW geometry (`growInstanced`): the next draw caches the new capacity (#018 B1).
   */
  private grow(b: Batch, need: number): void {
    let cap = b.capacity;
    while (cap < need) cap *= 2;
    const next = growInstanced(b.mesh, cap, true);
    b.geometry = next;
    b.rows = [0, 1, 2].map(
      (i) => next.getAttribute(`aInst${String(i)}`) as InstancedBufferAttribute,
    );
    b.base = next.getAttribute('aPalBase') as InstancedBufferAttribute;
    if (b.hairColor) b.hairColor = next.getAttribute('aHairColor') as InstancedBufferAttribute;
    b.capacity = cap;
  }

  private disposeBatch(b: Batch): void {
    b.mesh.removeFromParent();
    // the position, normal, uv, joints and index belong to the movers' own geometry: detached before `dispose` (#018 B1)
    releaseInstancedGeometry(b.geometry);
    b.material.dispose();
    b.pass = -1; // the records of its meshes are stale from now on (`batchFor` checks)
  }

  /** Draw calls the batches cost now. */
  get drawCalls(): number {
    return this.stats.batches;
  }

  /** The batches alive (tests, probe): key and instance count. */
  batchList(): { key: string; count: number }[] {
    return [...this.batches.values()].map((b) => ({ key: b.key, count: b.count }));
  }

  /**
   * The batches whose geometry asks for more instances than three.js will draw (#018 B1: the limit is cached at the first draw).
   * Always empty while every growth goes through `growInstanced`; the Playwright check and the tests read it.
   */
  instanceLimitViolations(): { name: string; count: number; limit: number }[] {
    const out: { name: string; count: number; limit: number }[] = [];
    for (const b of this.batches.values()) {
      if (exceedsInstanceLimit(b.geometry)) {
        out.push({
          name: b.key,
          count: b.geometry.instanceCount,
          limit: instanceLimit(b.geometry),
        });
      }
    }
    return out;
  }

  dispose(): void {
    for (const b of this.batches.values()) this.disposeBatch(b);
    this.batches.clear();
    for (const mesh of this.prevSet) {
      const k = (mesh.material as ModelMaterial).userData.key as MaterialKey | undefined;
      if (k) mesh.layers.set(LAYER[k.queue]);
    }
    this.prevSet.clear();
    this.nowSet.clear();
  }
}
