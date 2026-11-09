// One drawable instance of a model (Plan/41 sections 7, 11): the rigid hierarchy of a LOD group as three.js groups
// (`localTM` per GMOBJECT, parents precede children), one mesh per material block with the material of the D3D9
// system, texture binding through the `TextureCache` (the per-instance name rules `-etNN`, skin set, `_Flash`), and
// the layer of the pass group of each block. Used by the dev pages, previews, ground items and controls; static
// objects and movers get instanced / batched paths in WP-REND-11/12.
//
// Skinned objects are drawn in BIND POSE here (identity palette: `pos = v`); WP-REND-04 adds the palette skinning.
// Node animation (doors, chests) is pose-able with `setNodeFrame` (the original drops `localTM` while animating).
//
// Materials are shared between blocks with the same permutation, texture and constant alpha
// (`MaterialFactory.shared`); the block meshes of a model that waits for a texture are hidden until it settled. A texture that
// is already resident is applied in the same synchronous call (`TextureCache.peekReady`), so a new instance costs no frame
// without it, and a LOD change keeps the old level shown until every block of the new one is settled (BFIX1 B8, #018).

import { Group, type Matrix4, Mesh, Quaternion, Vector3, type Texture } from 'three';
import { LAYER } from '../layers';
import type { TextureHandle, TextureCache } from '../gl/textures';
import {
  deriveMaterial,
  Geo,
  type FogMode,
  type MaterialKey,
  keyString,
} from '../gl/materials/key';
import type {
  HairBinding,
  MaterialFactory,
  ModelMaterial,
  SkinBinding,
} from '../gl/materials/MaterialFactory';
import { modelTexturePath, textureCandidates, type TextureNameRules } from '../gl/textureNames';
import type { ModelGeometry, ObjectGeometry } from './ModelCache';
import type { Prio } from '../../workers/priority';

export interface ModelInstanceDeps {
  readonly materials: MaterialFactory;
  readonly textures: TextureCache;
}

export interface ModelInstanceOptions {
  /** LOD group to show first (default 0; files without LOD groups always use group 0). */
  readonly lod?: number;
  /** Texture name rules of this instance (texture set, skin set, eye blink). */
  readonly rules?: TextureNameRules;
  /** The model blend factor 0..255 (255 = opaque). */
  readonly modelBlend?: number;
  /** Extra `XE_*` bits of the element (`XE_REFLECT` of shields/weapons, ...). */
  readonly elementEffect?: number;
  readonly fog?: FogMode;
  readonly texturePrio?: Prio;
  readonly textureDistance?: number;
  /**
   * Palette skinning (WP-REND-04): skinned blocks get their own materials that read the bone palette of this
   * binding (`skin.base.value` = the first matrix of the mover). Without it skinned objects are drawn in the bind
   * pose with shared materials.
   */
  readonly skin?: SkinBinding;
  /** The element is the hair of a player: skinned blocks use the hair ambient rule (Plan/41 section 12.5). */
  readonly hair?: HairBinding;
  /**
   * Own materials for every block (the GLOW meshes of the effects, WP-REND-19): called for each block at build and whenever its
   * texture or constant alpha changes. The material it returns belongs to the instance (not shared): it is disposed when the
   * block gets another one and with the instance. Without it the blocks use the shared materials of the factory.
   */
  readonly materialFor?: (key: MaterialKey, map: Texture | null, alpha: number) => ModelMaterial;
}

/** A second texture of one block that can be switched on and off without a frame without texture (eye blink). */
export interface AlternateTexture {
  /** Shows the alternate texture (`true`) or the base texture. Takes effect at once when the alternate is resident. */
  set(active: boolean): void;
  readonly active: boolean;
  /** Settles when the alternate texture left the loading state. */
  readonly ready: Promise<void>;
}

/** One drawn block. */
export interface BlockMesh {
  readonly mesh: Mesh;
  readonly object: ObjectGeometry;
  readonly blockIndex: number;
  /** The current material key (changes with `setModelBlend`). */
  key: MaterialKey;
  /** Constant alpha factor of the current material. */
  alpha: number;
  /** The texture the block shows (the shared white until its handle settled). */
  map: Texture | null;
  /** The handle of the base texture (`null` for untextured blocks). */
  base: TextureHandle | null;
}

interface LodNodes {
  readonly root: Group;
  readonly nodes: Group[];
  readonly blocks: BlockMesh[];
}

/** One registered alternate texture (all LOD groups share it). */
interface AlternateSlot {
  readonly objectIndex: number;
  readonly blockIndex: number;
  readonly rules: TextureNameRules;
  handle: TextureHandle | null;
  active: boolean;
  readonly ready: Promise<void>;
}

const Q_A = new Quaternion();
const Q_B = new Quaternion();
const V_P = new Vector3();
const V_Q = new Vector3();
const ONE = new Vector3(1, 1, 1);

export class ModelInstance {
  /** Attach this to the scene graph; the instance transform goes on `root.matrix` (see `setTransform`). */
  readonly root = new Group();
  private readonly lods = new Map<number, LodNodes>();
  private current = -1;
  /** The LOD group waiting for its textures (built hidden; the shown level stays until it settled), `-1` = none. */
  private pendingLod = -1;
  private blend: number;
  private night = false;
  private readonly handles: TextureHandle[] = [];
  private readonly pending: Promise<unknown>[] = [];
  private readonly alternates: AlternateSlot[] = [];
  private disposed = false;

  constructor(
    readonly model: ModelGeometry,
    private readonly deps: ModelInstanceDeps,
    private readonly options: ModelInstanceOptions = {},
  ) {
    this.blend = options.modelBlend ?? 255;
    this.root.matrixAutoUpdate = false;
    this.root.name = model.key;
    this.setLod(options.lod ?? 0);
  }

  /** The blocks of the current LOD group. */
  get blocks(): readonly BlockMesh[] {
    return this.lods.get(this.current)?.blocks ?? [];
  }

  /** The LOD group shown. */
  get lod(): number {
    return this.current;
  }

  /** The LOD group that will replace the shown one as soon as its textures settled (`setLod` while they load), `-1` = none. */
  get nextLod(): number {
    return this.pendingLod;
  }

  /**
   * `true` when the shown level exists and every block of it is settled: untextured blocks count as settled, a `failed` texture
   * too (it shows white), a `loading` or `lost` one does not. A caller that swaps this instance for another way of drawing
   * the object waits for it, so the object is never missing for a frame.
   */
  get drawable(): boolean {
    const level = this.lods.get(this.current);
    return level !== undefined && this.levelSettled(level);
  }

  /** Settles when every texture requested so far is `ready` or `failed` (never rejects). */
  get ready(): Promise<void> {
    return Promise.all(this.pending).then(() => undefined);
  }

  /**
   * Sets the instance transform (three.js space, column-major elements of the finished world matrix). The world
   * matrices of the subtree are computed HERE: the renderer's scene does not traverse the graph every frame
   * (`matrixWorldAutoUpdate = false`), a static instance costs nothing per frame.
   */
  setTransform(m: Matrix4): void {
    this.root.matrix.copy(m);
    this.root.matrixWorldNeedsUpdate = true;
    this.root.updateMatrixWorld(true);
  }

  /**
   * Shows LOD group `lod` (clamped; built on first use). The FIRST level (the constructor) is shown at once. A later level is
   * built hidden: it replaces the shown one at once when every block of it is settled, else it becomes `nextLod` and the swap
   * happens when its last texture settles; the old level stays visible meanwhile (no frame without the object). A newer call
   * replaces a pending level; asking for the shown level cancels it.
   */
  setLod(lod: number): void {
    const want = Math.min(Math.max(0, Math.trunc(lod)), Math.max(0, this.model.lodCount - 1));
    if (want === this.current) {
      this.pendingLod = -1;
      return;
    }
    if (this.current !== -1 && want === this.pendingLod) return;
    let next = this.lods.get(want);
    if (!next) {
      next = this.buildLod(want);
      next.root.visible = false;
      this.lods.set(want, next);
      this.root.add(next.root);
      this.root.updateMatrixWorld(true);
    }
    if (this.current === -1 || this.levelSettled(next)) {
      this.show(want, next);
    } else {
      this.pendingLod = want;
    }
  }

  /** Makes `level` the shown one (hides the previous). */
  private show(lod: number, level: LodNodes): void {
    const cur = this.lods.get(this.current);
    if (cur && cur !== level) cur.root.visible = false;
    level.root.visible = true;
    this.current = lod;
    this.pendingLod = -1;
  }

  /** The block is settled: untextured, or its texture left `loading`/`lost` (a `failed` one shows white). */
  private blockSettled(bm: BlockMesh): boolean {
    const name = bm.object.blocks[bm.blockIndex]?.submesh.texture;
    if (name === null || name === undefined || name === '') return true;
    const state = bm.base?.state;
    return state === 'ready' || state === 'failed';
  }

  private levelSettled(level: LodNodes): boolean {
    return level.blocks.every((bm) => this.blockSettled(bm));
  }

  /** Swaps in the pending level once its last block settled. */
  private checkPending(): void {
    if (this.disposed || this.pendingLod < 0) return;
    const level = this.lods.get(this.pendingLod);
    if (level && this.levelSettled(level)) this.show(this.pendingLod, level);
  }

  private buildLod(lod: number): LodNodes {
    const objects = this.model.lods[lod] ?? [];
    const root = new Group();
    root.matrixAutoUpdate = false;
    root.name = `lod${String(lod)}`;
    const nodes: Group[] = [];
    const blocks: BlockMesh[] = [];
    for (const obj of objects) {
      const node = new Group();
      node.matrixAutoUpdate = false;
      if (obj.kind === 'rigid') node.matrix.copy(obj.localTM);
      if (obj.anim && obj.anim.length >= 7) this.poseNode(node, obj, 0, 0, 0);
      nodes.push(node);
      const parent = obj.parent.kind === 'object' ? nodes[obj.parent.index] : undefined;
      (parent ?? root).add(node);
      for (const b of obj.blocks) {
        const bm = this.buildBlock(obj, b.blockIndex);
        node.add(bm.mesh);
        blocks.push(bm);
      }
    }
    return { root, nodes, blocks };
  }

  private deriveFor(obj: ObjectGeometry, blockIndex: number): { key: MaterialKey; alpha: number } {
    const sub = obj.blocks[blockIndex]?.submesh;
    const d = deriveMaterial({
      effect: sub?.effect ?? 0,
      amount: sub?.amount ?? 255,
      light: obj.light,
      skinned: obj.kind === 'skin',
      elementEffect: this.options.elementEffect ?? 0,
      modelBlend: this.blend,
      ...(this.options.fog !== undefined ? { fog: this.options.fog } : {}),
      ...(this.options.hair ? { hair: true } : {}),
    });
    return { key: d.key, alpha: d.alphaScale };
  }

  /** `true` for the blocks that read the bone palette (their materials are this instance's own). */
  private palettized(key: MaterialKey): boolean {
    return this.options.skin !== undefined && key.geo === Geo.Skinned;
  }

  private materialFor(key: MaterialKey, map: Texture | null, alpha: number): ModelMaterial {
    if (this.options.materialFor) return this.options.materialFor(key, map, alpha);
    if (this.palettized(key)) {
      return this.deps.materials.create(key, {
        map,
        alpha,
        ...(this.options.skin ? { skin: this.options.skin } : {}),
        ...(this.options.hair ? { hair: this.options.hair } : {}),
      });
    }
    return this.deps.materials.shared(key, map, alpha);
  }

  private buildBlock(obj: ObjectGeometry, blockIndex: number): BlockMesh {
    const block = obj.blocks[blockIndex];
    if (!block) throw new Error(`block ${String(blockIndex)} missing`);
    const { key, alpha } = this.deriveFor(obj, blockIndex);
    const material = this.materialFor(key, null, alpha);
    const mesh = new Mesh(block.geometry, material);
    mesh.layers.set(this.layerFor(key));
    mesh.matrixAutoUpdate = false;
    // the bounding box of skinned geometry is the bind pose: the caller culls whole movers
    if (this.palettized(key)) mesh.frustumCulled = false;
    mesh.name = `${this.model.key}#${String(obj.index)}.${String(blockIndex)}`;
    mesh.userData['nightOnly'] = key.nightOnly;
    const bm: BlockMesh = { mesh, object: obj, blockIndex, key, alpha, map: null, base: null };
    this.bindTexture(bm, block.submesh.texture);
    return bm;
  }

  private bindTexture(bm: BlockMesh, textureName: string | null): void {
    if (textureName === null || textureName === '') return; // untextured: white (KEEP-QUIRK)
    bm.mesh.visible = false; // not drawn until the texture settled (Plan/41 section 22.4)
    const names = textureCandidates(textureName, this.options.rules ?? {}).map((n) =>
      modelTexturePath(n),
    );
    const acquire = this.acquireOptions();
    // a resident texture is applied in this call (BFIX1 B8): no frame without it
    const resident = this.deps.textures.peekReady(names, 'world', acquire);
    if (resident) {
      this.adopt(bm, resident);
      return;
    }
    const p = this.deps.textures.acquireFirst(names, 'world', acquire).then((h) => {
      if (this.disposed) {
        h.release();
        return;
      }
      this.adopt(bm, h);
    });
    this.pending.push(p);
  }

  private acquireOptions(): { prio?: Prio; distance?: number } {
    return {
      ...(this.options.texturePrio !== undefined ? { prio: this.options.texturePrio } : {}),
      ...(this.options.textureDistance !== undefined
        ? { distance: this.options.textureDistance }
        : {}),
    };
  }

  /** Takes ownership of the base texture handle of `bm`, applies it and follows its changes. */
  private adopt(bm: BlockMesh, h: TextureHandle): void {
    this.handles.push(h);
    bm.base = h;
    this.applyTexture(bm, h);
    h.onChange(() => {
      if (!this.disposed) this.applyTexture(bm, h);
    });
  }

  /** The pass layer of a key; night-only blocks sit in the hidden layer while it is day. */
  private layerFor(key: MaterialKey): number {
    return key.nightOnly && !this.night ? LAYER.hidden : LAYER[key.queue];
  }

  /**
   * Light objects (`ObjectMeta.light`) are drawn only at night (Plan/41 section 7.3): by day their blocks are moved
   * into the hidden layer, so they cost no draw call (the vertex shader keeps an early-out as a safety net).
   */
  setNight(night: boolean): void {
    if (night === this.night) return;
    this.night = night;
    for (const lod of this.lods.values()) {
      for (const bm of lod.blocks) {
        if (bm.key.nightOnly) bm.mesh.layers.set(this.layerFor(bm.key));
      }
    }
  }

  private setMaterial(bm: BlockMesh, key: MaterialKey, map: Texture | null, alpha: number): void {
    const old = bm.mesh.material as ModelMaterial;
    if (this.palettized(key) && keyString(key) === keyString(bm.key)) {
      // an own material of the same permutation: swap the uniforms, no new material
      const u = old.uniforms;
      if (u['uMap'] && map) u['uMap'].value = map;
      if (u['uAlpha']) u['uAlpha'].value = alpha;
      bm.alpha = alpha;
      bm.map = map;
      return;
    }
    const next = this.materialFor(key, map, alpha);
    bm.mesh.material = next;
    this.deps.materials.releaseShared(old);
    bm.mesh.layers.set(this.layerFor(key));
    bm.mesh.userData['nightOnly'] = key.nightOnly;
    bm.key = key;
    bm.alpha = alpha;
    bm.map = map;
  }

  private applyTexture(bm: BlockMesh, h: TextureHandle): void {
    if (h.state === 'loading' || h.state === 'lost') {
      bm.mesh.visible = false;
      return;
    }
    const map = this.mapOf(bm, h.texture);
    if (bm.map !== map) this.setMaterial(bm, bm.key, map, bm.alpha);
    bm.mesh.visible = true;
    this.checkPending();
  }

  /** The texture a block shows: the alternate while it is active and resident, else `base`. */
  private mapOf(bm: BlockMesh, base: Texture): Texture {
    for (const a of this.alternates) {
      if (
        a.active &&
        a.objectIndex === bm.object.index &&
        a.blockIndex === bm.blockIndex &&
        a.handle !== null &&
        a.handle.state === 'ready'
      ) {
        return a.handle.texture;
      }
    }
    return base;
  }

  /**
   * Registers an alternate texture for block `blockIndex` of object `objectIndex` (every LOD group): the base
   * texture name with `rules` applied (the closed-eyes `_Flash` of a head, Plan/41 section 12.5). Both textures stay
   * referenced, so switching is a uniform swap without a frame without texture (PL-137: per INSTANCE, the shared
   * geometry and the other instances are untouched).
   */
  addAlternate(objectIndex: number, blockIndex: number, rules: TextureNameRules): AlternateTexture {
    const first = this.model.lods[0]?.[objectIndex]?.blocks[blockIndex]?.submesh.texture ?? null;
    let resolve: () => void = () => undefined;
    const ready = new Promise<void>((r) => {
      resolve = r;
    });
    const slot: AlternateSlot = {
      objectIndex,
      blockIndex,
      rules,
      handle: null,
      active: false,
      ready,
    };
    this.alternates.push(slot);
    if (first === null || first === '') {
      resolve();
    } else {
      const names = textureCandidates(first, rules).map((n) => modelTexturePath(n));
      const acquire = this.acquireOptions();
      const resident = this.deps.textures.peekReady(names, 'world', acquire);
      if (resident) {
        this.handles.push(resident);
        slot.handle = resident;
        this.refreshSlot(slot);
        resolve();
      } else {
        const p = this.deps.textures
          .acquireFirst(names, 'world', acquire)
          .then((h) => {
            if (this.disposed) {
              h.release();
              return;
            }
            this.handles.push(h);
            slot.handle = h;
            this.refreshSlot(slot);
          })
          .finally(resolve);
        this.pending.push(p);
      }
    }
    const refresh = (): void => {
      this.refreshSlot(slot);
    };
    return {
      set(active) {
        if (slot.active === active) return;
        slot.active = active;
        refresh();
      },
      get active() {
        return slot.active;
      },
      ready,
    };
  }

  private refreshSlot(slot: AlternateSlot): void {
    for (const lod of this.lods.values()) {
      for (const bm of lod.blocks) {
        if (bm.object.index !== slot.objectIndex || bm.blockIndex !== slot.blockIndex) continue;
        if (bm.base === null || bm.base.state === 'loading') continue;
        this.applyTexture(bm, bm.base);
      }
    }
  }

  /**
   * Changes the model blend factor (fade, TRANSPARENT_MODE): blocks move between the pass groups and get the
   * matching material (Plan/41 section 7.3).
   */
  setModelBlend(blend: number): void {
    const b = Math.min(255, Math.max(0, Math.trunc(blend)));
    if (b === this.blend) return;
    this.blend = b;
    for (const lod of this.lods.values()) {
      for (const bm of lod.blocks) {
        const { key, alpha } = this.deriveFor(bm.object, bm.blockIndex);
        if (keyString(key) === keyString(bm.key) && alpha === bm.alpha) continue;
        this.setMaterial(bm, key, bm.map, alpha);
      }
    }
  }

  /**
   * Sets the matrix of the node group of object `objectIndex` in the current LOD group (relative to its parent
   * node): objects parented to a BONE of the mover skeleton get `bone[b] x localTM` per frame (Plan/41 section 12.4).
   */
  setNodeMatrix(objectIndex: number, m: Matrix4): void {
    const lod = this.lods.get(this.current);
    const node = lod?.nodes[objectIndex];
    if (!node) return;
    node.matrix.copy(m);
    node.matrixWorldNeedsUpdate = true;
    node.updateMatrixWorld();
  }

  /** The node group of object `objectIndex` of the current LOD group (bone-driven nodes, tests). */
  nodeOf(objectIndex: number): Group | null {
    return this.lods.get(this.current)?.nodes[objectIndex] ?? null;
  }

  /** Poses the node animation (rigid, node-animated files) at `frame` (the original drops `localTM` there). */
  setNodeFrame(frame: number): void {
    const lod = this.lods.get(this.current);
    if (!lod) return;
    const maxFrame = this.model.maxFrame;
    if (maxFrame <= 0) return;
    const f = Math.min(maxFrame - 1, Math.max(0, Math.floor(frame)));
    const s = frame - Math.floor(frame);
    const next = f + 1 >= maxFrame ? 0 : f + 1;
    const objects = this.model.lods[this.current] ?? [];
    objects.forEach((obj, i) => {
      const node = lod.nodes[i];
      if (node && obj.anim) this.poseNode(node, obj, f, next, s);
    });
  }

  private poseNode(node: Group, obj: ObjectGeometry, f: number, next: number, s: number): void {
    const a = obj.anim;
    if (!a) return;
    const ia = f * 7;
    const ib = next * 7;
    if (ia + 7 > a.length) return;
    Q_A.set(a[ia] ?? 0, a[ia + 1] ?? 0, a[ia + 2] ?? 0, a[ia + 3] ?? 1);
    V_P.set(a[ia + 4] ?? 0, a[ia + 5] ?? 0, a[ia + 6] ?? 0);
    if (s > 0 && ib + 7 <= a.length) {
      Q_B.set(a[ib] ?? 0, a[ib + 1] ?? 0, a[ib + 2] ?? 0, a[ib + 3] ?? 1);
      Q_A.slerp(Q_B, s);
      V_Q.set(a[ib + 4] ?? 0, a[ib + 5] ?? 0, a[ib + 6] ?? 0);
      V_P.lerp(V_Q, s);
    }
    node.matrix.compose(V_P, Q_A, ONE);
    node.matrixWorldNeedsUpdate = true;
    node.updateMatrixWorld();
  }

  /** Releases the texture references and the materials (the shared geometry belongs to the `ModelCache`). */
  dispose(): void {
    this.disposed = true;
    this.pendingLod = -1;
    for (const h of this.handles) h.release();
    this.handles.length = 0;
    for (const lod of this.lods.values()) {
      for (const bm of lod.blocks)
        this.deps.materials.releaseShared(bm.mesh.material as ModelMaterial);
    }
    this.root.removeFromParent();
    this.lods.clear();
  }
}
