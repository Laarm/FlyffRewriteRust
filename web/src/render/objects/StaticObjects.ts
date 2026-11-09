// Static objects of the lands (Plan/41 section 11, WP-REND-11): the `LndMeta.objects` placements, instanced per model
// across the resident lands, with the distance classes, the LOD groups and the occluding-object fade of the original.
//
//   * model resolution: `WorldRegistry.objectModel(index)` (`mdlObj`): file, `distant`, `trans`, `model_type`; a missing
//     file skips the object (KQ-30, logged once); `MODELTYPE_SFX` placements are collected for the SFX runtime (REND-18) and
//     not drawn here; billboards are skipped;
//   * instancing (R7, T3 with the "per model across all resident lands" grouping): ONE `InstancedBufferGeometry` per
//     (model, LOD group, GMOBJECT, material block) sharing the model's vertex buffers; per-instance attributes are the
//     three columns of the 3 x 4 matrix in three.js space (`aInst0..2`, `M_world * M_local(chain)`); CPU compaction writes
//     only the visible instances (distance cull, frustum test of the bounding sphere, LOD group) and runs whenever the view
//     changed (any element of the view-projection matrix: position, direction, FOV, aspect), the object set or the options
//     changed (no thresholds: a cached set hid or showed props at the frustum edge wrongly, BFIX1 #018). The distance classes
//     measure to the sphere SURFACE (large objects next to the player never vanish); the LOD group and the blend sort use the
//     pivot distance. Every instanced growth goes through `gl/instancing.ts` (`growInstanced`): three.js caches the instance
//     limit of a geometry at its first draw;
//   * models that cannot be instanced (skinned parts, node animation: the 134 animated "decor" objects) are drawn per
//     object as `ModelInstance`s. An animated one is registered with the SimWorld as a decor entry when it is first drawn
//     (`DECOR_ADD`, kind 0 static, the built-in clip, LOOP | VISIBLE | SOUNDS | SOUND_POSITIONAL; the node clip of the model
//     goes to the sim once), toggled with `DECOR_VISIBLE` when it enters or leaves the drawn set and dropped with `DECOR_REMOVE`
//     when its land is released (Plan/41 11.7, WP-REND-23); its node matrices come from the decor area of the render view
//     (`updateDecor`) and it is created and kept at LOD 0 (the node clip holds LOD 0 only and the LOD groups of some models, the
//     Flaris houses, list their objects in another order: LOD-0 matrices must never be applied to another group, #022). Until
//     the entry has a pose it stands in the bind pose;
//   * objects with `trans` fade per tick (`m_bTrans`): `blendFactor -10 / +10 / +5`, min 64; a faded object leaves the
//     instanced path and is drawn as a translucent `ModelInstance` (pass 13), then returns;
//   * light objects (`ObjectMeta.light`) draw only at night (the group moves to the hidden layer by day).

import {
  InstancedBufferGeometry,
  Matrix4,
  Mesh,
  Sphere,
  Vector3,
  type InstancedBufferAttribute,
  type Box3,
  type Scene,
  type Texture,
} from 'three';
import { OT_OBJ } from '../../gen/defs/define';
import { DECOR as DECOR_BITS } from '../../gen/sim/wire';
import { Prio } from '../../workers/priority';
import type { RenderCamera } from '../camera';
import type { GpuAccounting, UploadQueue } from '../gl/uploadQueue';
import { deriveMaterial, keyString, type MaterialKey } from '../gl/materials/key';
import type { MaterialFactory, ModelMaterial } from '../gl/materials/MaterialFactory';
import { modelTexturePath, textureCandidates } from '../gl/textureNames';
import type { TextureCache, TextureHandle } from '../gl/textures';
import { LAYER } from '../layers';
import type { RenderOptions } from '../quality';
import type { PlacementData } from '../terrain/landData';
import type { Vec3LH } from '../types';
import {
  AssetNotFoundError,
  type ModelSource,
  type ObjectModelInfo,
  type WorldRegistry,
} from '../world/ports';
import {
  createInstancedAttribute,
  exceedsInstanceLimit,
  growInstanced,
  instanceLimit,
  releaseInstancedGeometry,
} from '../gl/instancing';
import { cullDistance, lodGroupFor, PROCESS_RADIUS, stepFade } from './distanceTable';
import { mirrorMatrix } from '../space';
import { DecorView, nodeLocals, type DecorSink, type NodeParent } from './decor';
import { makeInstanced } from './instancedShader';
import type { ModelCache, ModelGeometry, ObjectGeometry } from './ModelCache';
import { ModelInstance } from './ModelInstance';
import { placementAt, worldMatrixThree, worldSphere, writeInstance } from './placement';

/** `MODELTYPE_*` of the registry. */
export const MODELTYPE = { mesh: 1, animated: 2, billboard: 3, sfx: 4 } as const;

export interface StaticDeps {
  readonly scene: Scene;
  readonly materials: MaterialFactory;
  readonly textures: TextureCache;
  readonly models: ModelCache;
  readonly source: ModelSource;
  readonly registry: WorldRegistry;
  readonly uploads: UploadQueue;
  readonly accounting: GpuAccounting;
  readonly options: () => RenderOptions;
  readonly indoor: boolean;
  readonly mpu: number;
  /** The sim side of the animated objects (resolved at each use: the movers may attach after the world); absent = bind pose. */
  readonly decor?: () => DecorSink | undefined;
  readonly log?: (message: string) => void;
}

interface ObjRec {
  readonly landKey: number;
  readonly model: ModelRec;
  readonly matrix: Matrix4;
  /** Position (three.js space) for distance. */
  readonly px: number;
  readonly py: number;
  readonly pz: number;
  /** Bounding sphere (three.js space). */
  readonly cx: number;
  readonly cy: number;
  readonly cz: number;
  readonly r: number;
  /** The sphere was computed from the model box (`fillSphere`); until then `r` is 0 at the pivot. */
  sphereFilled: boolean;
  blend: number;
  /** A `ModelInstance` for the individual path (fading or non-instanceable models). */
  instance: ModelInstance | null;
  /** Distance from the camera to the PIVOT in the last compaction (the LOD group and the blend sort use it). */
  distance: number;
  /** The decor entry of the sim (`0` = none), a client-local id. */
  decorId: number;
  /** What the sim was told last (`null` = nothing yet). */
  decorVisible: boolean | null;
  /** `frame_cur` of the entry in the last frame (probes), `-1` without a pose. */
  decorFrame: number;
}

interface Group {
  readonly lod: number;
  readonly object: ObjectGeometry;
  readonly blockIndex: number;
  readonly mesh: Mesh;
  /** Replaced by `growInstanced` when the group grows (`mesh.geometry` is the same object). */
  geometry: InstancedBufferGeometry;
  attrs: InstancedBufferAttribute[];
  readonly key: MaterialKey;
  capacity: number;
  count: number;
  texture: TextureHandle | null;
  /** The block's texture settled (or it has none): the group may draw. */
  ready: boolean;
  // ---- render-audio-m7 (WP-REND-27): what a texture override needs to bind the block again
  /** The base texture name of the block (`null` = none) and the alpha scale of its material. */
  readonly textureName: string | null;
  readonly alpha: number;
}

interface ModelRec {
  readonly index: number;
  readonly info: ObjectModelInfo;
  readonly path: string;
  state: 'loading' | 'ready' | 'failed';
  geometry: ModelGeometry | null;
  instanced: boolean;
  /** Composed GMOBJECT chain matrices per LOD group (three.js space), for the instanced path. */
  chains: Matrix4[][];
  groups: Group[];
  readonly objects: ObjRec[];
  /** The model has a node animation (`maxFrame > 0` with a node clip): its objects are decor entries of the sim. */
  animated: boolean;
  /** Node parents of the LOD-0 objects (the local matrices of the decor pose). */
  nodes: NodeParent[];
  /** The node clip was handed to the sim. */
  clipSent: boolean;
  /** `true` once the geometry has been "uploaded" (the upload job ran). */
  resident: boolean;
  abort: AbortController | null;
  /** The land with the highest priority that wants this model. */
  prio: Prio;
  /** Releases the asset manager's handle of the decoded model. */
  release: (() => void) | undefined;
  /** render-audio-m7: the texture file (under `Model/Texture`) every block of this model shows instead of its own, `null` = its own (the room papering). */
  override: string | null;
}

interface LandRec {
  readonly key: number;
  readonly x: number;
  readonly z: number;
  readonly objects: ObjRec[];
  readonly models: Set<ModelRec>;
  /** Placements of `MODELTYPE_SFX` objects (REND-18), LH position and angle. */
  readonly sfx: { index: number; pos: Vec3LH; angle: number; scale: Vec3LH }[];
}

const TMP_C = { center: new Vector3(), radius: 0 };

/** The occluder ray of the fade aims this high above the player's feet (his chest). */
const CHEST_HEIGHT = 1;

/** The lower-case file name of a model path. */
function basenameOf(path: string): string {
  return (path.split(/[\\/]/).pop() ?? '').toLowerCase();
}

/** The smallest power of two that is at least `n` (1 for `n <= 1`). */
function nextPow2(n: number): number {
  let p = 1;
  while (p < n) p *= 2;
  return p;
}

export class StaticObjects {
  private readonly models = new Map<number, ModelRec>();
  private readonly decorView = new DecorView();
  private readonly scratch: Matrix4[] = [];
  private readonly tmpLocal = new Matrix4();
  private nextDecorId = 1;
  private readonly lands = new Map<number, LandRec>();
  private readonly materialCache = new Map<string, { material: ModelMaterial; refs: number }>();
  private readonly missingLogged = new Set<number>();
  private night = false;
  private dirty = true;
  /** `camera.viewProjection.elements` of the last compaction (NaN = never). */
  private readonly lastViewProjection = new Float64Array(16).fill(Number.NaN);
  private lastOptions = '';
  private frustumCulling = true;
  private ownLand: { x: number; z: number } | null = null;
  private activePos: Vec3LH | null = null;
  private disposed = false;
  /** Visible instances drawn by the instanced groups in the last compaction. */
  visibleInstances = 0;
  /** Compactions run since creation. */
  compactions = 0;
  /** Milliseconds of the last compaction. */
  lastCompactMs = 0;
  /** Objects whose model is missing or unsupported. */
  skipped = 0;

  constructor(private readonly deps: StaticDeps) {}

  // ---- lands --------------------------------------------------------------------------------------------

  /** Adds the placements of a land that just became resident. `priority` is the importance of the land. */
  addLand(
    x: number,
    z: number,
    placements: PlacementData,
    landsW: number,
    priority: { prio: Prio; distance: number },
  ): void {
    if (this.disposed) return;
    const key = x + z * landsW;
    if (this.lands.has(key)) this.removeLand(x, z, landsW);
    const land: LandRec = { key, x, z, objects: [], models: new Set(), sfx: [] };
    this.lands.set(key, land);
    const m = new Matrix4();
    for (let i = 0; i < placements.count; i++) {
      const index = placements.ids[i * 4 + 1] ?? 0;
      const info = this.deps.registry.objectModel(index);
      const p = placementAt(placements.xf, i);
      if (!info?.file) {
        this.skipped++;
        this.logMissing(index, info ? 'no model file' : 'unknown model index');
        continue;
      }
      if (info.modelType === MODELTYPE.sfx) {
        land.sfx.push({ index, pos: p.pos, angle: p.angle, scale: p.scale });
        continue;
      }
      if (info.modelType === MODELTYPE.billboard) {
        this.skipped++;
        this.logMissing(index, 'billboard models are not drawn');
        continue;
      }
      const rec = this.modelRec(index, info, info.file, priority);
      worldMatrixThree(p, m);
      const det = m.determinant();
      if (!Number.isFinite(det) || det === 0) {
        this.skipped++; // a zero scale draws nothing
        continue;
      }
      const e = m.elements;
      const obj: ObjRec = {
        landKey: key,
        model: rec,
        matrix: m.clone(),
        px: e[12],
        py: e[13],
        pz: e[14],
        cx: e[12],
        cy: e[13],
        cz: e[14],
        r: 0,
        sphereFilled: false,
        blend: 255,
        instance: null,
        distance: 0,
        decorId: 0,
        decorVisible: null,
        decorFrame: -1,
      };
      land.objects.push(obj);
      land.models.add(rec);
      rec.objects.push(obj);
      // whatever the upload state: the box is known as soon as the geometry is (a land added between the decode and the upload
      // job used to keep radius 0 at its pivot and was culled as soon as the pivot left the view, B2)
      if (rec.geometry) this.fillSphere(obj, rec.geometry);
    }
    this.dirty = true;
  }

  private logMissing(index: number, why: string): void {
    if (this.missingLogged.has(index)) return;
    this.missingLogged.add(index);
    this.deps.log?.(`static object model ${String(index)}: ${why}`);
  }

  removeLand(x: number, z: number, landsW: number): void {
    const key = x + z * landsW;
    const land = this.lands.get(key);
    if (!land) return;
    this.lands.delete(key);
    for (const o of land.objects) {
      this.disposeInstance(o);
      const arr = o.model.objects;
      const i = arr.indexOf(o);
      if (i >= 0) arr.splice(i, 1);
    }
    for (const rec of land.models) {
      if (rec.objects.length === 0) this.releaseModel(rec);
    }
    this.dirty = true;
  }

  /** The `MODELTYPE_SFX` placements of a land (the SFX runtime of WP-REND-18 reads them). */
  sfxPlacements(
    x: number,
    z: number,
    landsW: number,
  ): readonly { index: number; pos: Vec3LH; angle: number; scale: Vec3LH }[] {
    return this.lands.get(x + z * landsW)?.sfx ?? [];
  }

  // ---- models -------------------------------------------------------------------------------------------

  private modelRec(
    index: number,
    info: ObjectModelInfo,
    path: string,
    priority: { prio: Prio; distance: number },
  ): ModelRec {
    const have = this.models.get(index);
    if (have) {
      if (priority.prio < have.prio) have.prio = priority.prio;
      return have;
    }
    const rec: ModelRec = {
      index,
      info,
      path,
      state: 'loading',
      geometry: null,
      instanced: true,
      chains: [],
      groups: [],
      objects: [],
      animated: false,
      nodes: [],
      clipSent: false,
      resident: false,
      abort: new AbortController(),
      prio: priority.prio,
      release: undefined,
      override: this.overrides.get(basenameOf(path)) ?? null,
    };
    this.models.set(index, rec);
    this.deps.source.load(path, priority.prio, priority.distance, rec.abort?.signal).then(
      (decoded) => {
        if (this.disposed || rec.abort === null) {
          decoded.release?.();
          return;
        }
        rec.release = decoded.release;
        const geometry = this.deps.models.add(path, decoded.meta, decoded.blob);
        this.onModelLoaded(rec, geometry);
      },
      (e: unknown) => {
        if (rec.abort === null) return;
        rec.state = 'failed';
        rec.abort = null;
        if (!(e instanceof AssetNotFoundError)) {
          this.deps.log?.(
            `static model ${path} failed: ${e instanceof Error ? e.message : String(e)}`,
          );
        } else {
          this.logMissing(index, `file ${path} is missing`);
        }
        this.dirty = true;
      },
    );
    return rec;
  }

  private fillSphere(o: ObjRec, g: ModelGeometry): void {
    worldSphere(g.bbox, o.matrix, TMP_C);
    const rr = o as { cx: number; cy: number; cz: number; r: number; sphereFilled: boolean };
    rr.cx = TMP_C.center.x;
    rr.cy = TMP_C.center.y;
    rr.cz = TMP_C.center.z;
    rr.r = TMP_C.radius;
    rr.sphereFilled = true;
  }

  private onModelLoaded(rec: ModelRec, geometry: ModelGeometry): void {
    rec.abort = null;
    rec.geometry = geometry;
    // can it be instanced? rigid, not node-animated, in every LOD group
    rec.instanced = geometry.lods.every((g) =>
      g.every((o) => o.kind === 'rigid' && o.anim === null),
    );
    // a node-animated model (the 134 animated "decor" objects) is driven by the sim through a decor entry. Only models the registry
    // declares ANIMATED_MESH animate (`CModel::IsAniable`, the OT_ANI bucket of `CLandscape::AddObjArray`, lod.cpp:1545): 15 `obj`
    // models are declared MESH although their file holds node keys (the Saint Morning town hall, the Marse stone gate, Flaris
    // buildings...). The original draws those still, in frame 0 (`ModelInstance` poses every node at key 0 and drops `localTM`).
    const aniable = rec.info.modelType === MODELTYPE.animated;
    rec.animated = aniable && geometry.nodeClip !== null && geometry.maxFrame > 0 && !rec.instanced;
    rec.nodes = (geometry.lods[0] ?? []).map((ob) => ({
      parent: ob.parent.kind === 'object' ? ob.parent.index : -1,
    }));
    for (const o of rec.objects) this.fillSphere(o, geometry);
    // the "upload" of the geometry buffers: counted against the per-frame budget, then the groups appear
    this.deps.uploads.enqueue({
      key: `static-model:${rec.path}`,
      prio: rec.prio,
      distance: 0,
      bytes: geometry.gpuBytes,
      run: () => {
        if (this.disposed || rec.geometry !== geometry) return;
        rec.resident = true;
        rec.state = 'ready';
        for (const o of rec.objects) this.fillSphere(o, geometry);
        if (rec.instanced) this.buildGroups(rec, geometry);
        this.dirty = true;
      },
    });
  }

  /** Composes the `localTM` chain of every object of every LOD group (parents precede children). */
  private chainsOf(geometry: ModelGeometry): Matrix4[][] {
    return geometry.lods.map((objects) => {
      const out: Matrix4[] = [];
      for (const obj of objects) {
        const local = obj.kind === 'rigid' ? obj.localTM : new Matrix4();
        const parent = obj.parent.kind === 'object' ? out[obj.parent.index] : undefined;
        out.push(parent ? parent.clone().multiply(local) : local.clone());
      }
      return out;
    });
  }

  private buildGroups(rec: ModelRec, geometry: ModelGeometry): void {
    rec.chains = this.chainsOf(geometry);
    const rules = rec.info.textureEx > 0 ? { textureSet: rec.info.textureEx } : {};
    // most groups never grow; growth stays correct (`growInstanced`)
    const capacity = Math.max(16, nextPow2(rec.objects.length));
    geometry.lods.forEach((objects, lod) => {
      for (const obj of objects) {
        for (const block of obj.blocks) {
          const sub = block.submesh;
          const derived = deriveMaterial({
            effect: sub.effect,
            amount: sub.amount,
            light: obj.light,
            skinned: false,
          });
          const key: MaterialKey = { ...derived.key, instanced: true };
          const g = new InstancedBufferGeometry();
          g.setAttribute('position', block.geometry.getAttribute('position'));
          g.setAttribute('normal', block.geometry.getAttribute('normal'));
          g.setAttribute('uv', block.geometry.getAttribute('uv'));
          const index = block.geometry.index;
          if (index) g.setIndex(index);
          g.setDrawRange(sub.start, sub.count);
          const attrs = [0, 1, 2].map(() => createInstancedAttribute(capacity, 4));
          attrs.forEach((a, i) => g.setAttribute(`aInst${String(i)}`, a));
          g.instanceCount = 0;
          // a sphere that covers any instance: culling is done on the CPU per instance
          g.boundingSphere = new Sphere(new Vector3(), Infinity);
          const material = this.materialFor(key, derived.alphaScale, null);
          const mesh = new Mesh(g, material);
          mesh.frustumCulled = false;
          mesh.matrixAutoUpdate = false;
          mesh.visible = false;
          mesh.name = `static:${rec.path}#${String(obj.index)}.${String(block.blockIndex)}.l${String(lod)}`;
          mesh.layers.set(this.layerFor(key));
          mesh.userData['nightOnly'] = key.nightOnly;
          const group: Group = {
            lod,
            object: obj,
            blockIndex: block.blockIndex,
            mesh,
            geometry: g,
            attrs,
            key,
            capacity,
            count: 0,
            texture: null,
            ready: false,
            textureName: sub.texture ?? null,
            alpha: derived.alphaScale,
          };
          rec.groups.push(group);
          this.deps.scene.add(mesh);
          this.acquireGroupTexture(rec, group, rules);
        }
      }
    });
  }

  /** The block's texture (the override first, the texture-set rule next, the base texture last) acquired and bound to the group. */
  private acquireGroupTexture(rec: ModelRec, group: Group, rules: { textureSet?: number }): void {
    // ---- render-audio-m7: an override names ONE file under `Model/Texture` and applies to EVERY block (`SetTexture`), also to a block that has none of its own (the room papering)
    const names =
      rec.override !== null
        ? [modelTexturePath(rec.override)]
        : group.textureName
          ? textureCandidates(group.textureName, rules).map((n) => modelTexturePath(n))
          : null;
    if (names) {
      const alpha = group.alpha;
      const adopt = (h: TextureHandle): void => {
        group.texture = h;
        this.bindTexture(group, h.texture, alpha);
        h.onChange(() => {
          if (!this.disposed && group.texture === h) this.bindTexture(group, h.texture, alpha);
        });
      };
      // a resident texture is bound in this call: a rebuilt group draws in the frame it appears (B8)
      const resident = this.deps.textures.peekReady(names, 'world', { prio: Prio.Statics });
      if (resident) {
        adopt(resident);
      } else {
        void this.deps.textures.acquireFirst(names, 'world', { prio: Prio.Statics }).then((h) => {
          if (this.disposed || !rec.groups.includes(group)) {
            h.release();
            return;
          }
          adopt(h);
        });
      }
    } else {
      group.texture = null;
      this.bindTexture(group, null, group.alpha);
    }
  }

  // ---- render-audio-m7 (WP-REND-27): the room papering ------------------------------------------------------------------------------
  /** The texture overrides by model file name (lower case) that models loaded later start with. */
  private readonly overrides = new Map<string, string | null>();

  /**
   * Every block of every instance of the model file `file` (`obj_miniwall01.o3d`, any directory and letter case) shows the texture `textureFile` (a name under `Model/Texture`) instead of its own;
   * `null` restores its own (`CGuildHouse::ApplyEFTexture`, `CDeployManager::ChangeWallTex`: `GetObject3D()->SetTexture`). Models that load later start with it. Returns the groups rebound
   * (the instanced path; a model that is drawn through individual instances is not covered). Scoped to the current world: the renderer clears it at every world change.
   */
  setTextureOverride(file: string, textureFile: string | null): number {
    const want = file.toLowerCase();
    if (textureFile === null) this.overrides.delete(want);
    else this.overrides.set(want, textureFile);
    let rebound = 0;
    for (const rec of this.models.values()) {
      if (basenameOf(rec.path) !== want || rec.override === textureFile) continue;
      rec.override = textureFile;
      const rules = rec.info.textureEx > 0 ? { textureSet: rec.info.textureEx } : {};
      for (const g of rec.groups) {
        g.texture?.release();
        g.texture = null;
        this.acquireGroupTexture(rec, g, rules);
        rebound++;
      }
    }
    this.dirty = true;
    return rebound;
  }

  /** The texture overrides in force (probe, tests): lower-case model file -> texture file. */
  overridesInForce(): ReadonlyMap<string, string | null> {
    return this.overrides;
  }

  /** Forgets every override (a world change); the blocks show their own textures again. */
  clearTextureOverrides(): void {
    for (const file of [...this.overrides.keys()]) this.setTextureOverride(file, null);
  }

  private bindTexture(group: Group, texture: Texture | null, alpha: number): void {
    const old = group.mesh.material as ModelMaterial;
    const next = this.materialFor(group.key, alpha, texture);
    group.mesh.material = next;
    this.releaseMaterial(old);
    group.ready = true;
    this.dirty = true;
  }

  private layerFor(key: MaterialKey): number {
    return key.nightOnly && !this.night ? LAYER.hidden : LAYER[key.queue];
  }

  /** Shared, instanced materials by (key, texture, alpha8). */
  private materialFor(key: MaterialKey, alpha: number, texture: Texture | null): ModelMaterial {
    const a8 = Math.round(Math.min(1, Math.max(0, alpha)) * 255);
    const id = `${keyString(key)}#${texture === null ? 'white' : String(texture.id)}#${String(a8)}`;
    let e = this.materialCache.get(id);
    if (!e) {
      const material = this.deps.materials.create(key, { map: texture, alpha: a8 / 255, name: id });
      makeInstanced(material, key);
      material.userData.sharedId = id;
      e = { material, refs: 0 };
      this.materialCache.set(id, e);
    }
    e.refs++;
    return e.material;
  }

  private releaseMaterial(m: ModelMaterial): void {
    const id = m.userData.sharedId;
    if (id === undefined) return;
    const e = this.materialCache.get(id);
    if (!e) return;
    e.refs--;
    if (e.refs <= 0) {
      this.materialCache.delete(id);
      e.material.dispose();
    }
  }

  private releaseModel(rec: ModelRec): void {
    rec.abort?.abort();
    rec.abort = null;
    for (const g of rec.groups) {
      this.deps.scene.remove(g.mesh);
      // not `geometry.dispose()`: that would also delete the GL buffers of the position/normal/uv/index attributes the group
      // shares with the `ModelCache` block geometries (and with the groups of another registry entry of the same file)
      releaseInstancedGeometry(g.geometry);
      this.releaseMaterial(g.mesh.material as ModelMaterial);
      g.texture?.release();
    }
    rec.groups.length = 0;
    if (rec.geometry) this.deps.models.release(rec.path);
    rec.release?.();
    rec.release = undefined;
    rec.geometry = null;
    rec.resident = false;
    this.models.delete(rec.index);
    this.deps.uploads.cancel(`static-model:${rec.path}`);
  }

  private disposeInstance(o: ObjRec): void {
    if (o.instance) {
      o.instance.dispose();
      o.instance = null;
    }
    if (o.decorId !== 0) {
      this.deps.decor?.()?.remove(o.decorId);
      o.decorId = 0;
      o.decorVisible = null;
      o.decorFrame = -1;
    }
  }

  // ---- state --------------------------------------------------------------------------------------------

  setNight(night: boolean): void {
    if (night === this.night) return;
    this.night = night;
    for (const rec of this.models.values()) {
      for (const g of rec.groups) {
        if (g.key.nightOnly) g.mesh.layers.set(this.layerFor(g.key));
      }
    }
    for (const rec of this.models.values()) {
      for (const o of rec.objects) o.instance?.setNight(night);
    }
  }

  /** Tells the system where the active mover is (the occluder test of the fade) and which land is "own". */
  setViewer(active: Vec3LH | null, ownLand: { x: number; z: number } | null): void {
    this.activePos = active;
    this.ownLand = ownLand;
  }

  /** Models of the land `(x, z)`: how many are wanted / loaded (the loading gate counts the own land). */
  landStatus(x: number, z: number, landsW: number): { wanted: number; ready: number } {
    const land = this.lands.get(x + z * landsW);
    if (!land) return { wanted: 0, ready: 0 };
    let ready = 0;
    for (const m of land.models) if (m.state !== 'loading') ready++;
    return { wanted: land.models.size, ready };
  }

  // ---- per frame ----------------------------------------------------------------------------------------

  /**
   * The cosmetic 60 Hz steps: the fade of `trans` objects within the processing radius of the camera. Needs the camera
   * (three.js position) and the active mover (set with `setViewer`).
   */
  tick(steps: number, camera: RenderCamera): void {
    if (steps <= 0 || !this.activePos) return;
    const cam = camera.position;
    const ax = this.activePos[0];
    const ay = this.activePos[1];
    const az = -this.activePos[2];
    // the "nearer" test compares the pivot with the FEET as in the original (Obj.cpp:246-282). The occluder ray, though, goes to
    // the CHEST and only counts hits in front of it: a floor plate the player stands on (the feet are inside its top face) or
    // an object behind the player never fades; only an object strictly between the camera and the player does (owner, #018)
    const ca = (ax - cam.x) ** 2 + (ay - cam.y) ** 2 + (az - cam.z) ** 2;
    const toChest = Math.hypot(ax - cam.x, ay + CHEST_HEIGHT - cam.y, az - cam.z) || 1;
    const dir: readonly [number, number, number] = [
      (ax - cam.x) / toChest,
      (ay + CHEST_HEIGHT - cam.y) / toChest,
      (az - cam.z) / toChest,
    ];
    for (const rec of this.models.values()) {
      if (!rec.info.trans || rec.state !== 'ready' || !rec.geometry) continue;
      for (const o of rec.objects) {
        const co = (o.px - cam.x) ** 2 + (o.py - cam.y) ** 2 + (o.pz - cam.z) ** 2;
        if (co > PROCESS_RADIUS * PROCESS_RADIUS) continue;
        const nearer = co < ca;
        if (!nearer && o.blend === 255) continue;
        // one ray test per object and frame batch: the geometry does not change between the steps
        const hit = nearer && this.rayHits(rec.geometry, o, cam.x, cam.y, cam.z, dir, toChest);
        let blend = o.blend;
        for (let s = 0; s < steps; s++) blend = stepFade(blend, nearer, hit);
        if (blend !== o.blend) {
          o.blend = blend;
          this.dirty = true;
        }
      }
    }
  }

  /**
   * `true` when the ray `origin + t * dir` (LH, unit `dir`, within `maxDist`) enters the bounding box of a resident object: the
   * occlusion test of the sun's lens flare (Plan/41 14.5, `PickObject_Fast`). Visual only; a bounding-sphere test rejects most
   * objects before the box test in the object's local space.
   */
  rayHitsBoxes(origin: Vec3LH, dir: Vec3LH, maxDist: number): boolean {
    const o = new Vector3(origin[0], origin[1], -origin[2]);
    const d = new Vector3(dir[0], dir[1], -dir[2]).normalize();
    const c = new Vector3();
    const lo = new Vector3();
    const ld = new Vector3();
    for (const rec of this.models.values()) {
      const g = rec.geometry;
      if (!g || rec.state !== 'ready') continue;
      for (const obj of rec.objects) {
        if (!(obj.r > 0)) continue;
        c.set(obj.cx - o.x, obj.cy - o.y, obj.cz - o.z);
        const t = c.dot(d);
        if (t < -obj.r || t > maxDist + obj.r) continue;
        if (c.lengthSq() - t * t > obj.r * obj.r) continue;
        const inv = obj.matrix.clone().invert();
        lo.copy(o).applyMatrix4(inv);
        ld.copy(d).transformDirection(inv).normalize();
        if (rayBox(g.bbox, lo, ld)) return true;
      }
    }
    return false;
  }

  /** The segment camera -> chest of the mover against the object's LOD-0 triangles (precise test, Obj.cpp `Pick`). */
  private rayHits(
    g: ModelGeometry,
    o: ObjRec,
    ox: number,
    oy: number,
    oz: number,
    dir: readonly [number, number, number],
    maxT: number,
  ): boolean {
    return rayHitsModel(g, o.matrix, [ox, oy, oz], dir, maxT);
  }

  /**
   * Draws one frame: culls, LODs and compacts the instanced groups, updates the individual instances. `nowMs` is
   * unused (the fades run per tick). Returns the number of drawn instances.
   */
  update(camera: RenderCamera): number {
    if (this.disposed) return 0;
    const pos = camera.position;
    const opts = this.deps.options();
    const optKey = `${String(opts.objectDistant)}${String(opts.objectDetail)}`;
    // recomputed whenever the view changed: any element of the view-projection matrix (position, direction, FOV, aspect), no
    // thresholds (a cached set left props at the frustum edge hidden or shown wrongly, #018). An unchanged camera produces
    // identical elements, so a still camera costs nothing.
    const vp = camera.viewProjection.elements;
    let viewChanged = false;
    for (let i = 0; i < 16; i++) {
      if (vp[i] !== this.lastViewProjection[i]) {
        viewChanged = true;
        break;
      }
    }
    if (!this.dirty && !viewChanged && optKey === this.lastOptions) return this.visibleInstances;
    const t0 = performance.now();
    this.lastViewProjection.set(vp);
    this.lastOptions = optKey;
    this.dirty = false;
    this.compactions++;
    const frustum = camera.frustum;
    const planes = frustum.planes;
    let drawn = 0;
    const lodCount = 3;
    for (const rec of this.models.values()) {
      if (rec.state !== 'ready' || !rec.geometry || !rec.resident) continue;
      const limit = cullDistance(opts.objectDistant, rec.info.distant);
      const perLod: ObjRec[][] = [[], [], []];
      for (const o of rec.objects) {
        // safety net of the sphere race: an object that never got its sphere gets it before the test
        if (!o.sphereFilled) this.fillSphere(o, rec.geometry);
        const dx = o.px - pos.x;
        const dy = o.py - pos.y;
        const dz = o.pz - pos.z;
        const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
        o.distance = dist; // the PIVOT distance: the LOD group and the far-to-near sort of blended groups
        // the distance class measures to the sphere surface (the original: the pivot, World3D.cpp:678): a large object whose
        // near side is next to the player is never cut
        const sx = o.cx - pos.x;
        const sy = o.cy - pos.y;
        const sz = o.cz - pos.z;
        if (Math.max(0, Math.sqrt(sx * sx + sy * sy + sz * sz) - o.r) > limit) {
          this.hideInstance(o);
          continue;
        }
        // frustum test of the bounding sphere (the plane normals point inwards)
        let inside = true;
        if (this.frustumCulling) {
          for (let i = 0; i < 6; i++) {
            const p = planes[i];
            if (!p) continue;
            if (p.normal.x * o.cx + p.normal.y * o.cy + p.normal.z * o.cz + p.constant < -o.r) {
              inside = false;
              break;
            }
          }
        }
        if (!inside) {
          this.hideInstance(o);
          continue;
        }
        const lod = Math.min(
          lodCount - 1,
          Math.min(
            rec.geometry.lodCount - 1,
            lodGroupFor(dist, opts.objectDetail, this.deps.indoor),
          ),
        );
        if (!rec.instanced) {
          this.drawIndividually(rec, o, lod);
          drawn++;
          continue;
        }
        if (o.blend < 255) {
          // a fading object leaves its group: its `ModelInstance` takes over once it is drawable; until then the group still
          // draws it (no frame without the object) and the next frame looks again
          const inst = this.drawIndividually(rec, o, lod);
          if (inst && !inst.drawable) {
            inst.root.visible = false;
            perLod[lod]?.push(o);
            this.dirty = true;
          }
          drawn++;
          continue;
        }
        this.hideInstance(o);
        perLod[lod]?.push(o);
        drawn++;
      }
      if (rec.instanced) this.fillGroups(rec, perLod);
    }
    this.visibleInstances = drawn;
    this.lastCompactMs = performance.now() - t0;
    return drawn;
  }

  /**
   * Test hook (default on): `false` skips only the sphere-plane test of `update()` (distance classes, LOD and fade unchanged).
   * The Playwright suite uses it as the oracle "nothing on screen was culled". Not used by the game.
   */
  setFrustumCulling(on: boolean): void {
    if (on === this.frustumCulling) return;
    this.frustumCulling = on;
    this.dirty = true;
  }

  private hideInstance(o: ObjRec): void {
    if (o.instance) o.instance.root.visible = false;
    this.setDecorVisible(o, false);
  }

  /** `DECOR_VISIBLE`: the sim evaluates the pose of an entry only while it is drawn (once per change). */
  private setDecorVisible(o: ObjRec, visible: boolean): void {
    if (o.decorId === 0 || o.decorVisible === visible) return;
    o.decorVisible = visible;
    this.deps.decor?.()?.setVisible(o.decorId, visible);
  }

  /**
   * Registers an animated object with the sim (`DECOR_ADD`) the first time it is drawn: the node clip of its model goes to the
   * sim as the built-in clip of `(OT_OBJ, index)`, the entry loops from frame 0 with sounds. Returns `false` when the motion table
   * has no built-in clip for the model (the object stays in its bind pose).
   */
  private registerDecor(rec: ModelRec, o: ObjRec): boolean {
    if (o.decorId !== 0) return true;
    const decor = this.deps.decor?.();
    const clip = rec.geometry?.nodeClip;
    if (!decor || !clip) return false;
    const clipId = decor.builtinClip(OT_OBJ, rec.index);
    if (clipId === null) return false;
    if (!rec.clipSent) {
      rec.clipSent = true;
      decor.loadClip(clipId, clip);
    }
    const m = mirrorMatrix(o.matrix.elements);
    const local = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10], m[12], m[13], m[14]].map(
      (v) => v ?? 0,
    );
    o.decorId = this.nextDecorId++;
    o.decorVisible = true;
    decor.add({
      id: o.decorId,
      kind: DECOR_BITS.KIND_STATIC,
      flags: DECOR_BITS.LOOP | DECOR_BITS.VISIBLE | DECOR_BITS.SOUNDS | DECOR_BITS.SOUND_POSITIONAL,
      attach: 0,
      modelType: OT_OBJ,
      modelIndex: rec.index,
      clipId: DECOR_BITS.CLIP_BUILTIN,
      skelId: 0,
      parentMover: 0,
      startFrame: 0,
      local,
    });
    return true;
  }

  /**
   * Applies the node matrices of the sim's decor entries to the drawn animated objects (every drawn frame: the clocks run whether
   * or not the camera moves). `view` is the render view of the frame; an object whose entry has no pose yet keeps the bind pose.
   */
  updateDecor(view: unknown): void {
    if (this.disposed) return;
    if (!this.decorView.bind(view)) return;
    const dv = this.decorView;
    for (const rec of this.models.values()) {
      if (!rec.animated) continue;
      for (const o of rec.objects) {
        const inst = o.instance;
        if (!inst?.root.visible) continue;
        // the sim side may attach after the object was first drawn (the movers come after the world): register it now
        if (o.decorId === 0 && !this.registerDecor(rec, o)) continue;
        const i = dv.find(o.decorId);
        o.decorFrame = i >= 0 && dv.hasPose(i) ? dv.frameCur(i) : -1;
        if (i < 0 || !dv.hasPose(i)) continue;
        // the node matrices are LOD-0 indexed: never write them into another group (skip the frame instead)
        if (inst.lod !== 0) {
          inst.setLod(0);
          if (inst.lod !== 0) continue;
        }
        nodeLocals(
          rec.nodes,
          (k, out) => dv.matrix(i, k, out),
          (n, local) => {
            this.tmpLocal.copy(local);
            inst.setNodeMatrix(n, this.tmpLocal);
          },
          this.scratch,
        );
      }
    }
  }

  /** The animated objects registered with the sim (tests, probes): model index, entry id, last frame, drawn, LOD shown. */
  decorProbe(): {
    index: number;
    id: number;
    frame: number;
    drawn: boolean;
    visible: boolean | null;
    lod: number;
  }[] {
    const out: {
      index: number;
      id: number;
      frame: number;
      drawn: boolean;
      visible: boolean | null;
      lod: number;
    }[] = [];
    for (const rec of this.models.values()) {
      for (const o of rec.objects) {
        if (o.decorId === 0) continue;
        out.push({
          index: rec.index,
          id: o.decorId,
          frame: o.decorFrame,
          drawn: o.instance?.root.visible === true,
          visible: o.decorVisible,
          lod: o.instance?.lod ?? -1,
        });
      }
    }
    return out;
  }

  /** Every object that has a `ModelInstance` (probes): registry index, animated, LOD shown, drawn now, drawable. */
  instanceProbe(): {
    index: number;
    animated: boolean;
    lod: number;
    drawn: boolean;
    drawable: boolean;
  }[] {
    const out: {
      index: number;
      animated: boolean;
      lod: number;
      drawn: boolean;
      drawable: boolean;
    }[] = [];
    for (const rec of this.models.values()) {
      for (const o of rec.objects) {
        if (!o.instance) continue;
        out.push({
          index: rec.index,
          animated: rec.animated,
          lod: o.instance.lod,
          drawn: o.instance.root.visible,
          drawable: o.instance.drawable,
        });
      }
    }
    return out;
  }

  /**
   * The groups whose geometry would draw fewer instances than `instanceCount` (three.js cached a smaller limit at the first
   * draw, `gl/instancing.ts`): `[]` when every visible instance is drawn. Probe for tests and the render suite.
   */
  instanceLimitViolations(): { name: string; count: number; limit: number }[] {
    const out: { name: string; count: number; limit: number }[] = [];
    for (const rec of this.models.values()) {
      for (const g of rec.groups) {
        if (exceedsInstanceLimit(g.geometry)) {
          out.push({
            name: g.mesh.name,
            count: g.geometry.instanceCount,
            limit: instanceLimit(g.geometry),
          });
        }
      }
    }
    return out;
  }

  /**
   * Bounding spheres of the objects of ready models (probe): how many there are, how many never got their sphere, and how
   * many have radius 0 although the model box has a size (an empty/inverted box or a lost race: they are culled with the pivot).
   */
  sphereProbe(): { objects: number; unfilled: number; zeroRadius: number } {
    let objects = 0;
    let unfilled = 0;
    let zeroRadius = 0;
    const size = new Vector3();
    for (const rec of this.models.values()) {
      if (rec.state !== 'ready' || !rec.geometry) continue;
      const sized = rec.geometry.bbox.getSize(size).lengthSq() > 0;
      for (const o of rec.objects) {
        objects++;
        if (!o.sphereFilled) unfilled++;
        if (sized && o.r === 0) zeroRadius++;
      }
    }
    return { objects, unfilled, zeroRadius };
  }

  /**
   * An object drawn through a `ModelInstance`: non-instanceable models and objects that are fading. Returns the instance.
   *
   * An animated object is created and kept at LOD 0: the node clip holds LOD 0 only (Plan/31 13.7), the sim's node matrices are
   * indexed by the objects of LOD group 0, and the LOD groups of some models (the Flaris houses) list their objects in another
   * order and with other parents, so LOD-0 matrices applied to another group fly, rotate and sink the bodies (#022). The
   * Plan/30 13.4 shortcut "reuse the LOD-0 matrices when the object counts match" is wrong for exactly these models.
   */
  private drawIndividually(rec: ModelRec, o: ObjRec, lod: number): ModelInstance | null {
    const g = rec.geometry;
    if (!g) return null;
    const group = rec.animated ? 0 : lod;
    if (!o.instance) {
      const rules = rec.info.textureEx > 0 ? { textureSet: rec.info.textureEx } : {};
      o.instance = new ModelInstance(
        g,
        { materials: this.deps.materials, textures: this.deps.textures },
        { lod: group, rules, modelBlend: o.blend, texturePrio: Prio.Statics },
      );
      o.instance.setNight(this.night);
      o.instance.setTransform(o.matrix);
      this.deps.scene.add(o.instance.root);
    }
    o.instance.root.visible = true;
    o.instance.setLod(group); // a no-op when unchanged
    o.instance.setModelBlend(o.blend);
    if (rec.animated && this.registerDecor(rec, o)) this.setDecorVisible(o, true);
    return o.instance;
  }

  /** Writes the instance buffers of every group of `rec`. */
  private fillGroups(rec: ModelRec, perLod: ObjRec[][]): void {
    const tmp = new Matrix4();
    const out = new Float32Array(12);
    if (rec.groups.some((g) => g.key.queue === 'blended')) {
      // blended blocks draw far to near (the original sorted whole objects by distance)
      for (const list of perLod) list.sort((a, b) => b.distance - a.distance);
    }
    for (const g of rec.groups) {
      const list = perLod[g.lod] ?? [];
      const chain = rec.chains[g.lod]?.[g.object.index];
      const n = g.ready ? list.length : 0;
      if (n > g.capacity) this.grow(g, n);
      const a0 = g.attrs[0];
      const a1 = g.attrs[1];
      const a2 = g.attrs[2];
      if (!a0 || !a1 || !a2) continue;
      for (let i = 0; i < n; i++) {
        const o = list[i];
        if (!o) continue;
        if (chain) tmp.multiplyMatrices(o.matrix, chain);
        else tmp.copy(o.matrix);
        writeInstance(tmp, out, 0);
        const w = i * 4;
        (a0.array as Float32Array).set(out.subarray(0, 4), w);
        (a1.array as Float32Array).set(out.subarray(4, 8), w);
        (a2.array as Float32Array).set(out.subarray(8, 12), w);
      }
      g.count = n;
      g.geometry.instanceCount = n;
      for (const a of g.attrs) {
        a.clearUpdateRanges();
        if (n > 0) a.addUpdateRange(0, n * 4);
        a.needsUpdate = n > 0;
      }
      g.mesh.visible = n > 0;
    }
  }

  /** Doubles the capacity of a group until `need` fit: a NEW geometry (`growInstanced`, three.js caches the limit per geometry). */
  private grow(g: Group, need: number): void {
    let cap = g.capacity;
    while (cap < need) cap *= 2;
    // `fillGroups` rewrites every instance right after: no data to keep
    const next = growInstanced(g.mesh, cap, false);
    g.geometry = next;
    g.attrs = [0, 1, 2].map(
      (i) => next.getAttribute(`aInst${String(i)}`) as InstancedBufferAttribute,
    );
    g.capacity = cap;
  }

  // ---- stats --------------------------------------------------------------------------------------------

  /** Draw groups with at least one instance (an upper bound of the static draw calls of the instanced path). */
  get drawGroups(): number {
    let n = 0;
    for (const rec of this.models.values()) for (const g of rec.groups) if (g.count > 0) n++;
    return n;
  }

  stats(): {
    objects: number;
    models: number;
    modelsReady: number;
    modelsFailed: number;
    groups: number;
    drawGroups: number;
    individual: number;
    fading: number;
    skipped: number;
    visibleInstances: number;
    compactions: number;
  } {
    let objects = 0;
    let ready = 0;
    let failed = 0;
    let groups = 0;
    let individual = 0;
    let fading = 0;
    for (const rec of this.models.values()) {
      objects += rec.objects.length;
      if (rec.state === 'ready') ready++;
      if (rec.state === 'failed') failed++;
      groups += rec.groups.length;
      for (const o of rec.objects) {
        if (o.instance?.root.visible) individual++;
        if (o.blend < 255) fading++;
      }
    }
    return {
      objects,
      models: this.models.size,
      modelsReady: ready,
      modelsFailed: failed,
      groups,
      drawGroups: this.drawGroups,
      individual,
      fading,
      skipped: this.skipped,
      visibleInstances: this.visibleInstances,
      compactions: this.compactions,
    };
  }

  /** Total placements of resident lands. */
  get objectCount(): number {
    let n = 0;
    for (const land of this.lands.values()) n += land.objects.length;
    return n;
  }

  /** The own land of the viewer (the loading gate's static models). */
  get own(): { x: number; z: number } | null {
    return this.ownLand;
  }

  // ---- housing / guild house (ui-house-quiz)
  /**
   * The placements of the model file `file` (`obj_miniwall01.o3d`, any letter case, any directory) in the resident lands whose model is loaded: the LH world matrix in D3D row-major order and the geometry.
   * The placement collision of the furniture ghost reads the room wall through it (`CWorld::ProcessObjCollision`); an empty list = the world has no such object (yet).
   */
  instancesOf(file: string): { matrix: number[]; geometry: ModelGeometry }[] {
    const want = file.toLowerCase();
    const out: { matrix: number[]; geometry: ModelGeometry }[] = [];
    for (const rec of this.models.values()) {
      const name = (rec.path.split(/[\\/]/).pop() ?? '').toLowerCase();
      if (name !== want || !rec.geometry) continue;
      for (const o of rec.objects) {
        // the stored matrix is the three.js one; the mirror is its own inverse
        out.push({ matrix: mirrorMatrix(Array.from(o.matrix.elements)), geometry: rec.geometry });
      }
    }
    return out;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const rec of [...this.models.values()]) {
      for (const o of rec.objects) this.disposeInstance(o);
      this.releaseModel(rec);
    }
    for (const e of this.materialCache.values()) e.material.dispose();
    this.materialCache.clear();
    this.lands.clear();
    this.models.clear();
  }
}

/**
 * `true` when the ray `(origin, dir)` (three.js world space, `dir` unit length) hits a triangle of the LOD-0 objects of
 * `g` placed with `matrix` (`0 < t <= maxT`; the original's `Pick` is an infinite ray, `maxT` defaults to infinity). Uses the CPU
 * pick copies of the model. `maxT` is in WORLD units: the local ray direction is re-normalised per object, so the world end
 * point `origin + dir * maxT` is transformed into each object's local space and `t` is compared with the local distance to it
 * (a scaled placement is still bounded in world units).
 */
export function rayHitsModel(
  g: ModelGeometry,
  matrix: Matrix4,
  origin: readonly [number, number, number],
  dir: readonly [number, number, number],
  maxT = Infinity,
): boolean {
  const inv = matrix.clone().invert();
  const o = new Vector3(origin[0], origin[1], origin[2]).applyMatrix4(inv);
  const d = new Vector3(dir[0], dir[1], dir[2]).transformDirection(inv).normalize();
  const end = Number.isFinite(maxT)
    ? new Vector3(
        origin[0] + dir[0] * maxT,
        origin[1] + dir[1] * maxT,
        origin[2] + dir[2] * maxT,
      ).applyMatrix4(inv)
    : null;
  const objects = g.lods[0] ?? [];
  const chain: Matrix4[] = [];
  for (const obj of objects) {
    const local = obj.kind === 'rigid' ? obj.localTM : new Matrix4();
    const parent = obj.parent.kind === 'object' ? chain[obj.parent.index] : undefined;
    chain.push(parent ? parent.clone().multiply(local) : local.clone());
  }
  const a = new Vector3();
  const b = new Vector3();
  const c = new Vector3();
  const e1 = new Vector3();
  const e2 = new Vector3();
  const p = new Vector3();
  const t = new Vector3();
  const q = new Vector3();
  for (let oi = 0; oi < objects.length; oi++) {
    const obj = objects[oi];
    const m = chain[oi];
    if (!obj || !m) continue;
    // the ray in the object's local space
    const mi = m.clone().invert();
    const lo = o.clone().applyMatrix4(mi);
    const ld = d.clone().transformDirection(mi);
    // the end of the segment in this object's local units
    const limit = end ? end.clone().applyMatrix4(mi).distanceTo(lo) : Infinity;
    // quick reject against the object's box
    if (!rayBox(obj.bbox, lo, ld, limit)) continue;
    const pos = obj.pick.positions;
    const idx = obj.pick.index;
    for (let i = 0; i + 2 < idx.length; i += 3) {
      const ia = (idx[i] ?? 0) * 3;
      const ib = (idx[i + 1] ?? 0) * 3;
      const ic = (idx[i + 2] ?? 0) * 3;
      a.set(pos[ia] ?? 0, pos[ia + 1] ?? 0, pos[ia + 2] ?? 0);
      b.set(pos[ib] ?? 0, pos[ib + 1] ?? 0, pos[ib + 2] ?? 0);
      c.set(pos[ic] ?? 0, pos[ic + 1] ?? 0, pos[ic + 2] ?? 0);
      // Moeller-Trumbore, both faces
      e1.subVectors(b, a);
      e2.subVectors(c, a);
      p.crossVectors(ld, e2);
      const det = e1.dot(p);
      if (Math.abs(det) < 1e-12) continue;
      const inv = 1 / det;
      t.subVectors(lo, a);
      const u = t.dot(p) * inv;
      if (u < 0 || u > 1) continue;
      q.crossVectors(t, e1);
      const v = ld.dot(q) * inv;
      if (v < 0 || u + v > 1) continue;
      const hitT = e2.dot(q) * inv;
      if (hitT > 0 && hitT <= limit) return true;
    }
  }
  return false;
}

/** Slab test of a ray against a box (`0 <= t <= maxT`, the infinite ray by default). */
function rayBox(box: Box3, o: Vector3, d: Vector3, maxT = Infinity): boolean {
  let tmin = 0;
  let tmax = maxT;
  for (const ax of ['x', 'y', 'z'] as const) {
    const inv = 1 / (d[ax] || 1e-30);
    let t0 = (box.min[ax] - o[ax]) * inv;
    let t1 = (box.max[ax] - o[ax]) * inv;
    if (t0 > t1) [t0, t1] = [t1, t0];
    tmin = Math.max(tmin, t0);
    tmax = Math.min(tmax, t1);
    if (tmin > tmax) return false;
  }
  return true;
}
