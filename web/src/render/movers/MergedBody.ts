// The runtime of merged parts (`merge.ts`): ONE merged skinned geometry of a mover per LOD group, a material per material
// state with the mover's own texture slots, the texture handles (with the name rules and the eye-blink alternate), the
// LOD switch and the model blend. A `MergedBody` is what a `ModelInstance` is for a single part: it draws 1-3 meshes for
// the head, the body parts, the armour and the cloak of a player together (Plan/41 section 12.7).
//
// LOD levels (#018 B8): the SHOWN level is the one drawn now, the PENDING one is built and waiting for its textures. A LOD change
// swaps in the same call when the new level is ready (every slot texture resident: `TextureCache.peekReady`), else the old level
// stays drawn until the textures arrive; a mover never has a frame without a body at a LOD step.

import { Group, Mesh, type Object3D, type Texture } from 'three';
import { LAYER } from '../layers';
import { deriveMaterial, keyString } from '../gl/materials/key';
import type { MaterialFactory, ModelMaterial, SkinBinding } from '../gl/materials/MaterialFactory';
import {
  applyTextureRules,
  modelTexturePath,
  textureCandidates,
  type TextureNameRules,
} from '../gl/textureNames';
import type { TextureCache, TextureHandle } from '../gl/textures';
import type { ModelGeometry } from '../objects/ModelCache';
import type { Prio } from '../../workers/priority';
import { MERGE_SLOTS, type MergedGeometry, type MergedGeometryCache } from './merge';

export interface MergedBodyDeps {
  readonly materials: MaterialFactory;
  readonly textures: TextureCache;
  readonly cache: MergedGeometryCache;
  readonly skin: SkinBinding;
}

export interface MergedPart {
  /** The model file (the cache key of the merged geometry). */
  readonly file: string;
  readonly model: ModelGeometry;
  /** `true` for the head part: object 0, block 0 gets the eye alternate texture. */
  readonly head?: boolean;
}

export interface MergedBodyOptions {
  readonly rules: TextureNameRules;
  readonly modelBlend: number;
  readonly lod: number;
  readonly texturePrio?: Prio;
}

interface Level {
  readonly geo: MergedGeometry;
  readonly uniforms: { value: Texture }[];
  readonly materials: Map<string, { material: ModelMaterial; state: string }>;
  readonly meshes: Mesh[];
  readonly group: Group;
  /** The slot of the head's eye block (`-1` when there is none). */
  eyeSlot: number;
  ready: boolean;
}

export class MergedBody {
  readonly root = new Group();
  private readonly levels = new Map<number, Level>();
  private readonly handles = new Map<string, TextureHandle>();
  private readonly eyes = new Map<string, TextureHandle>();
  /** The loads requested so far (`ready` settles when they all did). */
  private readonly waiting: Promise<unknown>[] = [];
  /** The LOD level drawn now (`-1`: none yet, the first level is shown when it is ready). */
  private shown = -1;
  /** The LOD level built and waiting for its textures, `-1` for none (the shown level stays drawn meanwhile). */
  private pending = -1;
  private blend: number;
  private closed = false;
  private disposed = false;
  private readonly files: string[];
  private readonly headIndex: number;

  constructor(
    private readonly d: MergedBodyDeps,
    private readonly parts: readonly MergedPart[],
    private readonly o: MergedBodyOptions,
    parent: Object3D,
  ) {
    this.blend = o.modelBlend;
    this.files = parts.map((p) => p.file);
    this.headIndex = parts.findIndex((p) => p.head === true);
    this.root.matrixAutoUpdate = false;
    this.root.name = 'merged-body';
    parent.add(this.root);
    this.setLod(o.lod);
  }

  /** Settles when every texture requested so far is resident (or failed). */
  get ready(): Promise<void> {
    return Promise.all(this.waiting).then(() => undefined);
  }

  /** The LOD group drawn now (`-1` before the first level is ready). */
  get lod(): number {
    return this.shown;
  }

  /** The LOD group waiting for its textures while `lod` stays drawn, `-1` for none (tests, probes). */
  get pendingLod(): number {
    return this.pending;
  }

  /** Draw calls of the shown LOD group (visible meshes). */
  drawCalls(): number {
    const lv = this.levels.get(this.shown);
    if (!lv?.ready) return 0;
    return lv.meshes.filter((m) => m.visible).length;
  }

  /** Calls `fn` for the visible meshes of the shown LOD group (the textures are resident). */
  forEachDrawnMesh(fn: (mesh: Mesh) => void): void {
    const lv = this.levels.get(this.shown);
    if (!lv?.ready || !lv.group.visible) return;
    for (const m of lv.meshes) if (m.visible) fn(m);
  }

  /** Vertices and triangles of the shown LOD group. */
  stats(): { vertices: number; triangles: number; meshes: number; slots: number } {
    const lv = this.levels.get(this.shown);
    return {
      vertices: lv?.geo.vertexCount ?? 0,
      triangles: lv?.geo.triangleCount ?? 0,
      meshes: lv?.meshes.length ?? 0,
      slots: lv?.geo.slotTextures.length ?? 0,
    };
  }

  /** The material states and slot names of the shown LOD (tests, probes). */
  slotNames(): readonly string[] {
    return this.levels.get(this.shown)?.geo.slotTextures ?? [];
  }

  /**
   * Asks for LOD group `lod` (clamped to the models' groups; built on first use). The new level is shown in this call when every
   * texture of its slots is resident; otherwise it becomes `pendingLod` and the level drawn now stays until the textures arrive
   * (#018 B8: no frame without a body at a LOD step). Asking for the shown level again forgets a pending one.
   */
  setLod(lod: number): void {
    const max = Math.max(...this.parts.map((p) => p.model.lodCount)) - 1;
    const want = Math.min(Math.max(0, Math.trunc(lod)), Math.max(0, max));
    if (want === this.shown) {
      this.pending = -1;
      return;
    }
    if (want === this.pending) return;
    let next = this.levels.get(want);
    if (!next) {
      const built = this.buildLevel(want);
      if (!built) return;
      this.levels.set(want, built);
      next = built;
    }
    if (next.ready) this.swapTo(want, next);
    else this.pending = want;
  }

  /** Hides the shown level, shows `next` (the level of LOD `want`): the one place where the shown level changes. */
  private swapTo(want: number, next: Level): void {
    const cur = this.levels.get(this.shown);
    if (cur) cur.group.visible = false;
    next.group.visible = true;
    this.shown = want;
    this.pending = -1;
  }

  private buildLevel(lod: number): Level | null {
    const geo = this.d.cache.acquire(
      this.files,
      this.parts.map((p) => p.model),
      lod,
    );
    if (!geo) return null;
    const white = this.d.textures.whiteTexture();
    const uniforms = Array.from({ length: MERGE_SLOTS }, () => ({ value: white }));
    const group = new Group();
    group.matrixAutoUpdate = false;
    group.name = `merged-lod${String(lod)}`;
    group.visible = false; // shown by `swapTo`, when the level is ready
    this.root.add(group);
    const level: Level = {
      geo,
      uniforms,
      materials: new Map(),
      meshes: [],
      group,
      eyeSlot: -1,
      ready: false,
    };
    if (this.headIndex >= 0) level.eyeSlot = geo.slotOf[this.headIndex]?.[0]?.[0] ?? -1;
    this.buildMeshes(level);
    // textures: one handle per distinct name (shared by the levels), applied to the slots of this level; a resident texture is
    // applied in this call (`acquireSlot` returns `null`), so a body over resident textures is ready without a microtask
    const waits: Promise<void>[] = [];
    geo.slotTextures.forEach((name, slot) => {
      const wait = this.acquireSlot(level, name, slot);
      if (wait) waits.push(wait);
    });
    if (waits.length === 0) {
      this.markReady(level);
      this.waiting.push(Promise.resolve());
    } else {
      this.waiting.push(
        Promise.all(waits).then(() => {
          if (this.disposed) return;
          this.markReady(level);
          // the level that was waiting for its textures replaces the shown one (the very first level has nothing to keep drawn)
          if (this.pending === lod) this.swapTo(lod, level);
        }),
      );
    }
    if (level.eyeSlot >= 0) {
      const name = geo.slotTextures[level.eyeSlot] ?? '';
      if (name !== '') this.waiting.push(this.acquireEye(name));
    }
    return level;
  }

  private markReady(level: Level): void {
    level.ready = true;
    for (const m of level.meshes) m.visible = true;
  }

  /** Meshes of every material state of the level (hidden until the textures settled). */
  private buildMeshes(level: Level): void {
    for (const m of level.meshes) m.removeFromParent();
    level.meshes.length = 0;
    level.geo.groups.forEach((g, i) => {
      const geo = level.geo.geometries[i];
      if (!geo) return;
      const d = deriveMaterial({
        effect: g.effect,
        amount: g.amount,
        light: false,
        skinned: true,
        modelBlend: this.blend,
      });
      const state = `${String(g.effect)}/${String(g.amount)}`;
      const material = this.d.materials.create(d.key, {
        alpha: d.alphaScale,
        skin: this.d.skin,
        slots: level.uniforms,
        name: `merged:${keyString(d.key)}`,
      });
      level.materials.set(state, { material, state });
      const mesh = new Mesh(geo, material);
      mesh.layers.set(LAYER[d.key.queue]);
      mesh.matrixAutoUpdate = false;
      mesh.frustumCulled = false; // the bounds are the bind pose; the mover is culled as a whole
      mesh.visible = level.ready;
      mesh.name = `merged#${String(i)}`;
      level.group.add(mesh);
      level.meshes.push(mesh);
    });
  }

  /**
   * Applies the texture of `name` to slot `slot` of `level`. Returns `null` when it was applied in this call (a resident or failed
   * handle of this body, or `TextureCache.peekReady` found the texture resident), else a promise that settles when it is.
   */
  private acquireSlot(level: Level, name: string, slot: number): Promise<void> | null {
    const u = level.uniforms[slot];
    if (name === '' || !u) return null;
    const known = this.handles.get(name);
    if (known && (known.state === 'ready' || known.state === 'failed')) {
      u.value = known.texture;
      return null;
    }
    const paths = textureCandidates(name, this.o.rules).map((n) => modelTexturePath(n));
    const options = this.o.texturePrio !== undefined ? { prio: this.o.texturePrio } : {};
    const peeked = this.d.textures.peekReady(paths, 'world', options);
    if (peeked) {
      this.keepHandle(name, peeked, u);
      return null;
    }
    return this.acquireSlotAsync(name, paths, options, u);
  }

  private async acquireSlotAsync(
    name: string,
    paths: readonly string[],
    options: { prio?: Prio },
    u: { value: Texture },
  ): Promise<void> {
    let h = this.handles.get(name);
    if (!h) {
      h = await this.d.textures.acquireFirst(paths, 'world', options);
      if (this.disposed) {
        h.release();
        return;
      }
    }
    this.keepHandle(name, h, u);
  }

  /** Stores the handle of `name` (a second one for the same name is released: the body holds one) and applies it to `u`. */
  private keepHandle(name: string, handle: TextureHandle, u: { value: Texture }): void {
    let h = handle;
    const existing = this.handles.get(name);
    if (existing && existing !== h) {
      h.release();
      h = existing;
    } else {
      this.handles.set(name, h);
    }
    if (h.state === 'loading' || h.state === 'lost') return;
    u.value = h.texture;
  }

  private async acquireEye(name: string): Promise<void> {
    if (this.eyes.has(name)) return;
    const flash = applyTextureRules(name, { ...this.o.rules, eyeFlash: true });
    const paths = [flash, name].map((n) => modelTexturePath(n));
    const h = await this.d.textures.acquireFirst(paths, 'world');
    if (this.disposed) {
      h.release();
      return;
    }
    this.eyes.set(name, h);
  }

  /** The closed-eyes texture (`_Flash`) on the head's eye block of every level, or the open one. */
  setEyesClosed(closed: boolean): void {
    if (this.closed === closed) return;
    this.closed = closed;
    for (const lv of this.levels.values()) {
      if (lv.eyeSlot < 0) continue;
      const name = lv.geo.slotTextures[lv.eyeSlot] ?? '';
      const u = lv.uniforms[lv.eyeSlot];
      if (!u || name === '') continue;
      const open = this.handles.get(name);
      const flash = this.eyes.get(name);
      const h = closed && flash?.state === 'ready' ? flash : open;
      if (h && (h.state === 'ready' || h.state === 'failed')) u.value = h.texture;
    }
  }

  /** `true` while the closed-eyes texture is shown. */
  get eyesClosed(): boolean {
    return this.closed;
  }

  /** The texture the head's eye block shows (probes, tests). */
  eyeTexture(): Texture | null {
    const lv = this.levels.get(this.shown);
    if (!lv || lv.eyeSlot < 0) return null;
    return lv.uniforms[lv.eyeSlot]?.value ?? null;
  }

  /** Changes the model blend factor (fade, TRANSPARENT_MODE): the material states get their new keys. */
  setModelBlend(blend: number): void {
    const b = Math.min(255, Math.max(0, Math.trunc(blend)));
    if (b === this.blend) return;
    this.blend = b;
    for (const lv of this.levels.values()) {
      for (const { material } of lv.materials.values()) material.dispose();
      lv.materials.clear();
      this.buildMeshes(lv);
    }
  }

  dispose(): void {
    this.disposed = true;
    for (const lod of this.levels.keys()) this.d.cache.release(this.files, lod);
    for (const lv of this.levels.values()) {
      for (const { material } of lv.materials.values()) material.dispose();
    }
    for (const h of this.handles.values()) h.release();
    for (const h of this.eyes.values()) h.release();
    this.handles.clear();
    this.eyes.clear();
    this.levels.clear();
    this.root.removeFromParent();
  }
}
