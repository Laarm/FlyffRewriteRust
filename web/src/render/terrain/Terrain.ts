// The terrain of a world (Plan/41 sections 9 and 10, WP-REND-08/09/10/16): owns the streamer, the land entities (mesh,
// splat materials, water and cloud meshes), the CPU height field and the shared animated water state, and draws them in
// the passes 2 (terrain), 9 (water and cloud sea) and 11 (underwater overlay).
//
// Life of a land: the streamer asks `load` (fetch + decode through the `LandSource`), then `build`:
//   1. typed views of the pack, the layer textures are acquired from the `TerrainTexturePool` and awaited;
//   2. layers whose texture failed are REMOVED (the next enabled layer becomes first; KEEP lod.cpp:1326-1340), the patch
//      masks and the alpha array are built for the kept layers;
//   3. two upload jobs (Plan/41 22.2: "large land uploads are split") run under the per-frame budget of the upload
//      queue: the textures (`renderer.initTexture`), then the geometry (the first draw uploads the buffers) and the
//      activation: the land enters the CPU height field, its border normals and its neighbours' are recomputed, the
//      meshes become visible and fade in over 250 ms (Plan/41 9.8, 22.4);
//   4. the land is READY (the gate counts it) and `onLandReady` hands its placements to the static objects.
// Eviction (outside the 5 x 5 retention window) disposes every three.js object, releases the texture handles, removes the
// heights and recomputes the neighbours' border normals.
//
// Every drawn frame `update` re-evaluates the streaming window around the ACTIVE position (the player), selects the lands
// within `r` lands of the CAMERA's land, updates the patch LOD levels and rebuilds the index buffers of the lands whose
// levels (or whose neighbours' facing levels) changed, ALL of them in the frame (no budget: a land that waited drew an old index
// buffer against the new levels of its neighbours, cracks along the seam, BFIX1 #015).

import {
  BufferAttribute,
  BufferGeometry,
  Matrix4,
  Mesh,
  Scene,
  type RawShaderMaterial,
  type WebGLRenderer,
} from 'three';
import { Prio } from '../../workers/priority';
import type { EnvUniforms } from '../gl/materials/env';
import type { TextureCache } from '../gl/textures';
import type { GpuAccounting, UploadQueue } from '../gl/uploadQueue';
import type { RenderOptions } from '../quality';
import type { Vec3LH, WorldRenderDesc } from '../types';
import type { DecodedLand, WorldAssets } from '../world/ports';
import { HeightField } from './heightField';
import {
  LIGHTMAP_TEXELS,
  MAP_SIZE,
  PATCHES_PER_SIDE,
  buildPatchMasks,
  landFromPack,
  waterPatches,
  type LandData,
} from './landData';
import { LandMesh } from './LandMesh';
import { computeNormals, recomputeBorderNormals } from './normals';
import { PatchLevels, levelRanges, type LevelAt, type PatchLevel } from './patchLod';
import {
  LAYERS_PER_PASS,
  createSplatMaterial,
  createSplatTextures,
  passCount,
  type SplatTextures,
} from './splatMaterial';
import { TerrainStreamer, landOf, visibilityRadius, type LandKey } from './streaming';
import type { TerrainTexHandle, TerrainTexturePool } from './terrainTexturePool';
import {
  CLOUD_LAYERS,
  buildCloudQuads,
  buildWaterQuads,
  isUnderwater,
  quadGeometry,
  waterSets,
} from './water';
import { WaterAnimator } from './waterAnimator';

/** The fade-in of a land that just became ready (Plan/41 9.8). */
export const FADE_MS = 250;

export interface TerrainDeps {
  readonly renderer: WebGLRenderer;
  readonly pool: Pick<TerrainTexturePool, 'acquire' | 'samplerTexture'>;
  readonly textures: TextureCache;
  readonly uploads: UploadQueue;
  readonly accounting: GpuAccounting;
  readonly env: EnvUniforms;
  readonly assets: WorldAssets;
  readonly options: () => RenderOptions;
  readonly world: WorldRenderDesc;
  /** A land became resident: its placements go to the static objects. */
  readonly onLandReady?: (land: LandData) => void;
  /** A land left the retention window. */
  readonly onLandEvicted?: (x: number, z: number) => void;
  /** Diagnostics (a layer was removed, a land failed). */
  readonly log?: (message: string) => void;
}

interface LandEntity {
  readonly key: number;
  readonly x: number;
  readonly z: number;
  readonly data: LandData;
  readonly mesh: LandMesh;
  readonly levels: PatchLevels;
  readonly meshes: Mesh[];
  readonly materials: RawShaderMaterial[];
  readonly textures: SplatTextures | null;
  readonly handles: TerrainTexHandle[];
  /** Water, cloud meshes and the geometries they own. */
  readonly extra: Mesh[];
  readonly geometries: BufferGeometry[];
  readonly bytes: number;
  /** Levels the index buffer was built for, and the neighbours' facing levels at that time (`-1` = none). */
  readonly builtLevels: Uint8Array;
  readonly builtBorder: Int8Array;
  indexBuilt: boolean;
  /** `performance.now()` of the activation, `null` while building. */
  fadeStart: number | null;
  fade: number;
  readonly waterSetsUsed: number[];
  hasCloud: boolean;
  hasWater: boolean;
  /** Releases the asset manager's handle of the decoded land. */
  readonly release: (() => void) | undefined;
  /** render-audio-m7: the bucket / slice uniforms of each splat pass as built, for `forceTexture(null)`. */
  readonly baseLayers: { readonly bucket: Float32Array; readonly slice: Float32Array }[];
}

/** The first pool id of the forced floor textures (outside the range of the real terrain texture ids): the pool keys its entries by id, so every distinct file gets its own. */
export const FORCED_TEXTURE_ID = 0x7fff_0001;

/** The index of the border level array: west 0-15, east 16-31, south 32-47, north 48-63. */
const BORDER = 64;

export class Terrain {
  /** Pass 2. */
  readonly scene = new Scene();
  /** Pass 9: water, then the cloud sea layers. */
  readonly waterScene = new Scene();
  /** Pass 11: the underwater overlay. */
  readonly overlayScene = new Scene();
  readonly field: HeightField;
  readonly streamer: TerrainStreamer<DecodedLand>;
  readonly water: WaterAnimator;
  private readonly lands = new Map<number, LandEntity>();
  private readonly pending = new Set<number>();
  private readonly folder: string;
  private readonly mpu: number;
  private readonly landsW: number;
  private readonly indoor: boolean;
  private underwaterOn = false;
  private disposed = false;
  /** Lands drawn in the last `update` (probe). */
  drawnLands = 0;
  /** The lands drawn in the last `update` (the seam probe). */
  private drawnEntities: LandEntity[] = [];
  /** Index rebuilds since creation (probe, the T2 cost). */
  indexRebuilds = 0;
  /** Milliseconds spent in index rebuilds in the last `update`. */
  lastRebuildMs = 0;
  /** `true` once the first `update` ran. */
  private started = false;
  /** The camera of the last `update` (a land that activates between two updates gets its levels from it). */
  private lastCamera: Vec3LH | null = null;

  constructor(private readonly deps: TerrainDeps) {
    const w = deps.world;
    this.folder = w.folder.toLowerCase();
    this.mpu = w.mpu;
    this.landsW = w.landsW;
    this.indoor = w.indoor;
    this.field = new HeightField(w.mpu, w.landsW, w.landsH);
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
    this.waterScene.matrixAutoUpdate = false;
    this.waterScene.matrixWorldAutoUpdate = false;
    this.overlayScene.matrixAutoUpdate = false;
    this.overlayScene.matrixWorldAutoUpdate = false;
    this.water = new WaterAnimator(deps.assets.registry, deps.textures, deps.env);
    this.streamer = new TerrainStreamer<DecodedLand>(
      w.landsW,
      w.landsH,
      w.mpu,
      new Set(w.present),
      {
        load: (req) =>
          deps.assets.lands.load({
            folder: this.folder,
            x: req.x,
            z: req.z,
            mpu: this.mpu,
            prio: req.prio,
            distance: req.distance,
            signal: req.signal,
          }),
        build: (x, z, data, isCurrent) => this.build(x, z, data, isCurrent),
        evict: (x, z) => {
          this.evict(x, z);
        },
        failed: (x, z, error) => {
          deps.log?.(
            `land ${String(x)}-${String(z)} failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        },
      },
    );
    this.initOverlay();
  }

  private key(x: number, z: number): number {
    return x + z * this.landsW;
  }

  // ---- queries ------------------------------------------------------------------------------------------

  /** Lands that are resident (built and visible or fading in). */
  get landsResident(): number {
    return this.lands.size;
  }

  /** `true` while `(x, z)` is resident. */
  has(x: number, z: number): boolean {
    return this.lands.has(this.key(x, z));
  }

  landData(x: number, z: number): LandData | null {
    return this.lands.get(this.key(x, z))?.data ?? null;
  }

  /** The loading gate's terrain part (Plan/40 9.8): the inner 3 x 3 present lands are ready. */
  gate(): { ready: number; wanted: number; open: boolean; progress: number } {
    return this.streamer.gate();
  }

  /** Estimated GPU bytes of the resident lands. */
  get gpuBytes(): number {
    let n = 0;
    for (const e of this.lands.values()) n += e.bytes;
    return n;
  }

  patchLevelHistogram(): [number, number, number, number] {
    const h: [number, number, number, number] = [0, 0, 0, 0];
    for (const e of this.lands.values()) {
      if (e.mesh.indexCount === 0) continue;
      const lh = e.levels.histogram();
      for (let i = 0; i < 4; i++) h[i] = (h[i] ?? 0) + (lh[i] ?? 0);
    }
    return h;
  }

  /** Total triangles of the land index buffers currently built. */
  get triangleCount(): number {
    let n = 0;
    for (const e of this.lands.values()) n += e.mesh.triangleCount;
    return n;
  }

  // ---- building -----------------------------------------------------------------------------------------

  private runJob(
    prio: Prio,
    distance: number,
    key: string,
    bytes: number,
    run: () => void,
  ): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      this.deps.uploads.enqueue({
        key,
        prio,
        distance,
        bytes,
        run: () => {
          try {
            run();
            resolve();
          } catch (e) {
            reject(e instanceof Error ? e : new Error(String(e)));
          }
        },
      });
    });
  }

  private async build(
    x: number,
    z: number,
    decoded: DecodedLand,
    isCurrent: () => boolean,
  ): Promise<void> {
    const data = landFromPack(decoded.meta, decoded.blob);
    const key = this.key(x, z);
    const gate = this.streamer.gateLands().some((l) => l.x === x && l.z === z);
    const prio = gate ? Prio.Critical : Prio.Near;
    const origin = this.streamer.position ?? { x: 0, z: 0 };
    const edge = MAP_SIZE * this.mpu;
    const distance = Math.hypot((x + 0.5) * edge - origin.x, (z + 0.5) * edge - origin.z);
    this.pending.add(key);
    const handles: TerrainTexHandle[] = [];
    let created = false;
    try {
      // 1. the layer textures
      const byId = new Map<number, TerrainTexHandle>();
      for (const layer of data.layers) {
        if (byId.has(layer.textureId)) continue;
        const path = this.deps.assets.registry.terrainFile(layer.textureId);
        if (path === null) {
          this.deps.log?.(
            `land ${String(x)}-${String(z)}: unknown terrain texture ${String(layer.textureId)}`,
          );
          continue;
        }
        const h = this.deps.pool.acquire(layer.textureId, path, { prio, distance });
        byId.set(layer.textureId, h);
        handles.push(h);
      }
      await Promise.all(handles.map((h) => h.ready));
      if (!isCurrent()) return;

      // 2. failed layers are removed
      const keep = (i: number): boolean => {
        const l = data.layers[i];
        return l !== undefined && byId.get(l.textureId)?.state === 'ready';
      };
      const masks = buildPatchMasks(data.layers, keep);
      if (masks.dropped > 0) {
        this.deps.log?.(
          `land ${String(x)}-${String(z)}: ${String(masks.dropped)} layers beyond 64 dropped`,
        );
      }
      const removed = data.layers.length - masks.kept.length - masks.dropped;
      if (removed > 0) {
        this.deps.log?.(
          `land ${String(x)}-${String(z)}: ${String(removed)} layers removed (texture failed)`,
        );
      }
      const keptCount = masks.kept.length;
      let alpha = new Uint8Array(0);
      let textures: SplatTextures | null = null;
      if (keptCount > 0) {
        alpha = new Uint8Array(keptCount * LIGHTMAP_TEXELS);
        masks.kept.forEach((src, k) => {
          alpha.set(
            data.alpha.subarray(src * LIGHTMAP_TEXELS, (src + 1) * LIGHTMAP_TEXELS),
            k * LIGHTMAP_TEXELS,
          );
        });
        textures = createSplatTextures(masks.data, alpha, keptCount, data.lightRgb);
      }
      const layerBucket = masks.kept.map(
        (i) => byId.get(data.layers[i]?.textureId ?? -1)?.bucket ?? 0,
      );
      const layerSlice = masks.kept.map(
        (i) => byId.get(data.layers[i]?.textureId ?? -1)?.slice ?? 0,
      );

      // 3. CPU objects (no GL yet)
      const mesh = new LandMesh(data);
      const entity = this.createEntity(
        x,
        z,
        data,
        mesh,
        textures,
        handles,
        masks.kept.length,
        { layerBucket, layerSlice },
        decoded.release,
      );
      created = true;
      const texBytes = textures
        ? 16 * 16 * 16 + keptCount * LIGHTMAP_TEXELS + LIGHTMAP_TEXELS * 4
        : 0;
      try {
        await this.runJob(prio, distance, `land-tex:${String(key)}`, texBytes, () => {
          if (!isCurrent() || !textures) return;
          this.deps.renderer.initTexture(textures.mask);
          this.deps.renderer.initTexture(textures.alpha);
          this.deps.renderer.initTexture(textures.light);
        });
        if (!isCurrent()) {
          this.disposeEntity(entity);
          return;
        }
        await this.runJob(prio, distance, `land-mesh:${String(key)}`, mesh.gpuBytes, () => {
          if (!isCurrent()) return;
          this.activate(entity);
        });
      } catch (e) {
        this.disposeEntity(entity);
        throw e;
      }
      if (!isCurrent()) {
        if (this.lands.get(key) === entity) this.removeEntity(entity);
        else this.disposeEntity(entity);
      }
    } finally {
      this.pending.delete(key);
      if (!created) {
        // the land never became an entity: nothing else will release its handles
        for (const h of handles) h.release();
        decoded.release?.();
      }
    }
  }

  private createEntity(
    x: number,
    z: number,
    data: LandData,
    mesh: LandMesh,
    textures: SplatTextures | null,
    handles: TerrainTexHandle[],
    keptCount: number,
    layers: { layerBucket: readonly number[]; layerSlice: readonly number[] },
    release: (() => void) | undefined,
  ): LandEntity {
    const world = new Matrix4().makeTranslation(mesh.originX, 0, -mesh.originZ);
    const meshes: Mesh[] = [];
    const materials: RawShaderMaterial[] = [];
    if (textures && keptCount > 0) {
      const buckets = Array.from({ length: 6 }, (_, i) => this.deps.pool.samplerTexture(i));
      const passes = passCount(keptCount);
      for (let p = 0; p < passes; p++) {
        const count = Math.min(LAYERS_PER_PASS, keptCount - p * LAYERS_PER_PASS);
        const mat = createSplatMaterial({
          textures,
          layerBucket: layers.layerBucket,
          layerSlice: layers.layerSlice,
          base: p * LAYERS_PER_PASS,
          count,
          mpu: this.mpu,
          env: this.deps.env,
          buckets,
          pass: p,
        });
        const m = new Mesh(mesh.geometry, mat);
        m.matrixAutoUpdate = false;
        m.matrix.copy(world);
        m.matrixWorld.copy(world);
        m.renderOrder = p;
        m.visible = false;
        m.name = `land-${String(x)}-${String(z)}-p${String(p)}`;
        meshes.push(m);
        materials.push(mat);
      }
    }
    // water and cloud sea
    const extra: Mesh[] = [];
    const geometries: BufferGeometry[] = [];
    const { water, cloud } = waterPatches(data.water);
    const sets = waterSets(water);
    for (const set of sets) {
      const q = buildWaterQuads(water, this.mpu, set);
      if (q.quads === 0) continue;
      const g = quadGeometry(q);
      geometries.push(g);
      const m = new Mesh(g, this.water.waterMaterial(set));
      m.matrixAutoUpdate = false;
      m.matrix.copy(world);
      m.matrixWorld.copy(world);
      m.renderOrder = 0;
      m.visible = false;
      m.userData['waterSet'] = set;
      m.name = `water-${String(x)}-${String(z)}-s${String(set)}`;
      extra.push(m);
    }
    if (cloud.length > 0) {
      const q = buildCloudQuads(cloud, this.mpu);
      const g = quadGeometry(q);
      geometries.push(g);
      CLOUD_LAYERS.forEach((layer, i) => {
        const cw = new Matrix4().makeTranslation(mesh.originX, layer.y, -mesh.originZ);
        const m = new Mesh(g, this.water.cloudMaterials[i === 0 ? 0 : 1]);
        m.matrixAutoUpdate = false;
        m.matrix.copy(cw);
        m.matrixWorld.copy(cw);
        m.renderOrder = i + 1;
        m.visible = false;
        m.userData['cloud'] = true;
        m.name = `cloud-${String(x)}-${String(z)}-${String(layer.y)}`;
        extra.push(m);
      });
    }
    const waterBytes = geometries.reduce(
      (n, g) =>
        n +
        ((g.getAttribute('position') as BufferAttribute | undefined)?.array.byteLength ?? 0) * 2,
      0,
    );
    const texBytes = textures
      ? 16 * 16 * 16 + keptCount * LIGHTMAP_TEXELS + LIGHTMAP_TEXELS * 4
      : 0;
    return {
      key: this.key(x, z),
      x,
      z,
      data,
      mesh,
      levels: new PatchLevels(),
      meshes,
      materials,
      textures,
      handles,
      extra,
      geometries,
      bytes: mesh.gpuBytes + texBytes + waterBytes,
      builtLevels: new Uint8Array(PATCHES_PER_SIDE * PATCHES_PER_SIDE),
      builtBorder: new Int8Array(BORDER).fill(-2),
      indexBuilt: false,
      fadeStart: null,
      fade: 0,
      waterSetsUsed: sets,
      hasCloud: cloud.length > 0,
      hasWater: water.length > 0,
      release,
      baseLayers: materials.map((m) => ({
        bucket: Float32Array.from(m.uniforms['uLayerBucket']?.value as Float32Array),
        slice: Float32Array.from(m.uniforms['uLayerSlice']?.value as Float32Array),
      })),
    };
  }

  // ---- render-audio-m7 (WP-REND-27): CLandscape::ForceTexture (lod.cpp:785-802) -------------------------------------------------------
  private forced: TerrainTexHandle | null = null;
  /** The pool id of each forced file. */
  private readonly forcedIds = new Map<string, number>();

  /**
   * Every visible layer of every land shows the texture file `path` (a VFS path, `model/texture/obj_houpapcar01.dds`): the splat passes sample one bucket / slice for all their layers, the
   * mask and alpha stay (the original swapped the texture object of each visible layer's terrain entry). `null` restores the layers' own textures. Lands that appear later start with it.
   * Scoped to this terrain: it is built per world, so the override ends with the world (FIX: the original overwrote the shared entry for good).
   */
  forceTexture(path: string | null): void {
    this.forced?.release();
    this.forced = null;
    if (path === null) {
      for (const e of this.lands.values()) this.applyLayers(e, null);
      return;
    }
    let id = this.forcedIds.get(path);
    if (id === undefined) {
      id = FORCED_TEXTURE_ID + this.forcedIds.size;
      this.forcedIds.set(path, id);
    }
    const handle = this.deps.pool.acquire(id, path, { prio: Prio.Critical });
    this.forced = handle;
    void handle.ready.then(() => {
      if (this.forced !== handle || this.disposed) return;
      if (handle.state !== 'ready') {
        this.deps.log?.(`floor texture ${path} could not be loaded`);
        return;
      }
      for (const e of this.lands.values()) this.applyLayers(e, handle);
    });
  }

  /** The texture a forced floor shows (probe, tests): its path, `null` without one. */
  get forcedPath(): string | null {
    return this.forced?.path ?? null;
  }

  private applyLayers(e: LandEntity, forced: TerrainTexHandle | null): void {
    e.materials.forEach((m, i) => {
      const bucket = m.uniforms['uLayerBucket']?.value as Float32Array | undefined;
      const slice = m.uniforms['uLayerSlice']?.value as Float32Array | undefined;
      const base = e.baseLayers[i];
      if (!bucket || !slice || !base) return;
      if (forced === null) {
        bucket.set(base.bucket);
        slice.set(base.slice);
      } else {
        bucket.fill(forced.bucket);
        slice.fill(forced.slice);
      }
      m.uniformsNeedUpdate = true;
    });
  }

  /** The upload job of the geometry: the land becomes resident. */
  private activate(e: LandEntity): void {
    if (this.disposed) {
      this.disposeEntity(e);
      return;
    }
    this.lands.set(e.key, e);
    if (this.forced?.state === 'ready') this.applyLayers(e, this.forced);
    this.field.setLand(e.x, e.z, { heights: e.data.heights, water: e.data.water });
    // normals over the global field (the neighbours that are resident count), then the neighbours' borders
    this.computeNormalsOf(e, false);
    for (const n of this.neighbours(e)) this.computeNormalsOf(n, true);
    for (const m of e.meshes) this.scene.add(m);
    for (const m of e.extra) this.waterScene.add(m);
    this.deps.accounting.set(`land:${String(e.x)},${String(e.z)}`, e.bytes);
    e.fadeStart = performance.now();
    e.fade = 0;
    for (const set of e.waterSetsUsed) this.water.waterMaterial(set);
    if (e.hasCloud) this.water.ensureCloud();
    if (e.hasWater) this.water.ensureUnderwater();
    // the first index buffer is built here so that the land never draws with an empty one, with the levels of the
    // current camera (all level 0 would be a one-frame spike of 32 000 triangles per land)
    if (this.lastCamera) {
      const opts = this.deps.options();
      e.levels.update(
        [this.lastCamera[0], this.lastCamera[1], this.lastCamera[2]],
        [e.mesh.originX, e.mesh.originZ],
        this.mpu,
        e.data.patchBounds,
        levelRanges(opts.terrainView, this.mpu, this.indoor),
      );
    }
    this.rebuildIndices(e);
    this.deps.onLandReady?.(e.data);
  }

  private computeNormalsOf(e: LandEntity, bordersOnly: boolean): void {
    const lookup = (vx: number, vz: number): number | undefined =>
      this.field.neighbourHeight(e.x, e.z, vx, vz);
    if (bordersOnly) recomputeBorderNormals(lookup, this.mpu, e.mesh.normals);
    else computeNormals(lookup, this.mpu, e.mesh.normals);
    e.mesh.normalsChanged();
  }

  private neighbours(e: LandEntity): LandEntity[] {
    const out: LandEntity[] = [];
    for (const [dx, dz] of [
      [-1, 0],
      [1, 0],
      [0, -1],
      [0, 1],
    ] as const) {
      const n = this.lands.get(this.key(e.x + dx, e.z + dz));
      if (n && n !== e) out.push(n);
    }
    return out;
  }

  /** Releases a land that left the retention window. */
  private evict(x: number, z: number): void {
    const e = this.lands.get(this.key(x, z));
    if (!e) return;
    this.removeEntity(e);
  }

  private removeEntity(e: LandEntity): void {
    this.lands.delete(e.key);
    this.field.setLand(e.x, e.z, null);
    for (const m of e.meshes) this.scene.remove(m);
    for (const m of e.extra) this.waterScene.remove(m);
    this.deps.accounting.remove(`land:${String(e.x)},${String(e.z)}`);
    this.disposeEntity(e);
    // the neighbours lose a neighbour: their border normals are one-sided again, their border patches see "no neighbour"
    for (const n of this.neighbours(e)) {
      this.computeNormalsOf(n, true);
      n.levels.invalidate();
    }
    this.deps.onLandEvicted?.(e.x, e.z);
  }

  private disposeEntity(e: LandEntity): void {
    for (const mat of e.materials) mat.dispose();
    e.mesh.dispose();
    e.textures?.mask.dispose();
    e.textures?.alpha.dispose();
    e.textures?.light.dispose();
    for (const g of e.geometries) g.dispose();
    for (const h of e.handles) h.release();
    e.handles.length = 0;
    e.release?.();
  }

  // ---- per frame ----------------------------------------------------------------------------------------

  /** The level of patch `(px, pz)` of land `e`, or of the neighbour land across its border. */
  private levelAtFor(e: LandEntity): LevelAt {
    return (px, pz) => {
      if (px >= 0 && pz >= 0 && px < PATCHES_PER_SIDE && pz < PATCHES_PER_SIDE) {
        return e.levels.at(px, pz);
      }
      const dx = px < 0 ? -1 : px >= PATCHES_PER_SIDE ? 1 : 0;
      const dz = pz < 0 ? -1 : pz >= PATCHES_PER_SIDE ? 1 : 0;
      if (dx !== 0 && dz !== 0) return undefined;
      const n = this.lands.get(this.key(e.x + dx, e.z + dz));
      if (!n || !n.indexBuilt || n.fadeStart === null) return undefined;
      return n.levels.at(
        ((px % PATCHES_PER_SIDE) + PATCHES_PER_SIDE) % PATCHES_PER_SIDE,
        ((pz % PATCHES_PER_SIDE) + PATCHES_PER_SIDE) % PATCHES_PER_SIDE,
      );
    };
  }

  /** The facing border levels of the neighbours as an array (`-1` = no neighbour), to detect changes. */
  private borderOf(e: LandEntity): Int8Array {
    const out = new Int8Array(BORDER);
    const at = this.levelAtFor(e);
    for (let i = 0; i < PATCHES_PER_SIDE; i++) {
      out[i] = at(-1, i) ?? -1;
      out[16 + i] = at(PATCHES_PER_SIDE, i) ?? -1;
      out[32 + i] = at(i, -1) ?? -1;
      out[48 + i] = at(i, PATCHES_PER_SIDE) ?? -1;
    }
    return out;
  }

  private needsRebuild(e: LandEntity): { rebuild: boolean; border: Int8Array } {
    const border = this.borderOf(e);
    if (!e.indexBuilt) return { rebuild: true, border };
    for (let i = 0; i < e.builtLevels.length; i++) {
      if (e.builtLevels[i] !== e.levels.levels[i]) return { rebuild: true, border };
    }
    for (let i = 0; i < BORDER; i++) {
      if (e.builtBorder[i] !== border[i]) return { rebuild: true, border };
    }
    return { rebuild: false, border };
  }

  /**
   * Seam probe (BFIX1 #015): over every pair of drawn, built neighbour lands (built = `indexBuilt` and `fadeStart !== null`, the
   * rule of `levelAtFor`), the number of border entries where the level one land's index was stitched against (`builtBorder`)
   * differs from the level the neighbour's index was built with on the facing edge (`builtLevels`), counted both ways. 0 means
   * every shared edge is watertight; a land that waited for its rebuild shows up here.
   */
  seamMismatches(): number {
    const drawn = new Set(this.drawnEntities);
    const built = (e: LandEntity): boolean => e.indexBuilt && e.fadeStart !== null;
    // border blocks of `builtBorder`: west 0, east 16, south 32, north 48; the facing patch of the neighbour for entry `i`
    const sides: readonly {
      block: number;
      dx: number;
      dz: number;
      facing: (i: number) => number;
    }[] = [
      { block: 0, dx: -1, dz: 0, facing: (i) => PATCHES_PER_SIDE - 1 + i * PATCHES_PER_SIDE },
      { block: 16, dx: 1, dz: 0, facing: (i) => i * PATCHES_PER_SIDE },
      { block: 32, dx: 0, dz: -1, facing: (i) => i + (PATCHES_PER_SIDE - 1) * PATCHES_PER_SIDE },
      { block: 48, dx: 0, dz: 1, facing: (i) => i },
    ];
    let mismatches = 0;
    for (const e of this.drawnEntities) {
      if (!built(e)) continue;
      for (const side of sides) {
        const n = this.lands.get(this.key(e.x + side.dx, e.z + side.dz));
        if (!n || !drawn.has(n) || !built(n)) continue;
        for (let i = 0; i < PATCHES_PER_SIDE; i++) {
          if (e.builtBorder[side.block + i] !== n.builtLevels[side.facing(i)]) mismatches++;
        }
      }
    }
    return mismatches;
  }

  private rebuildIndices(e: LandEntity): void {
    const t0 = performance.now();
    const { border } = this.needsRebuild(e);
    e.mesh.rebuildIndices(this.levelAtFor(e));
    e.builtLevels.set(e.levels.levels);
    e.builtBorder.set(border);
    e.indexBuilt = true;
    e.levels.dirty = false;
    this.indexRebuilds++;
    this.lastRebuildMs += performance.now() - t0;
  }

  /**
   * One drawn frame: streaming around `active` (the player, LH world position), drawing around `camera`.
   * `nowMs` is `performance.now()`.
   */
  update(camera: Vec3LH, active: Vec3LH, nowMs: number): void {
    if (this.disposed) return;
    this.started = true;
    this.lastCamera = camera;
    this.lastRebuildMs = 0;
    this.streamer.update({ x: active[0], z: active[2] }, nowMs);
    const opts = this.deps.options();
    const ranges = levelRanges(opts.terrainView, this.mpu, this.indoor);
    const camLand = landOf(camera[0], camera[2], this.mpu);
    const r = visibilityRadius(this.mpu);
    const drawn: LandEntity[] = [];
    for (const e of this.lands.values()) {
      const inRange = Math.abs(e.x - camLand.x) <= r && Math.abs(e.z - camLand.z) <= r;
      if (e.fadeStart !== null) {
        e.fade = Math.min(1, (nowMs - e.fadeStart) / FADE_MS);
      }
      for (const m of e.meshes) {
        m.visible = inRange;
        const u = (m.material as RawShaderMaterial).uniforms['uFade'];
        if (u) u.value = e.fade;
      }
      const wet = inRange;
      for (const m of e.extra) {
        const set = m.userData['waterSet'] as number | undefined;
        const ready = set !== undefined ? this.water.setReady(set) : this.water.cloudReady();
        m.visible = wet && ready;
      }
      if (inRange) drawn.push(e);
    }
    this.drawnLands = drawn.length;
    // patch levels of the drawn lands first, then the rebuild decisions (a neighbour's levels are final by then)
    const camPos: readonly [number, number, number] = [camera[0], camera[1], camera[2]];
    for (const e of drawn) {
      e.levels.update(
        camPos,
        [e.mesh.originX, e.mesh.originZ],
        this.mpu,
        e.data.patchBounds,
        ranges,
      );
    }
    // EVERY drawn land whose levels or seams changed is rebuilt in this frame (no per-frame budget: a land that waited kept an old
    // index buffer while its neighbours stitched against its NEW levels, T-junction cracks along the seam, #015). One pass is
    // exact: every drawn land is already built (`activate`), the levels of all of them were updated just above, and
    // `needsRebuild` reads only the land's own levels and its neighbours' levels / `indexBuilt` / `fadeStart`, none of which a
    // rebuild changes, so no decision of this pass depends on another rebuild of the same pass.
    this.drawnEntities = drawn;
    for (const e of drawn) {
      if (this.needsRebuild(e).rebuild) this.rebuildIndices(e);
    }
    this.water.update();
    // underwater overlay
    const w = this.field.waterAt(camera[0], camera[2]);
    const under = isUnderwater(w, camera[1]) && this.water.underwaterReady();
    this.underwaterOn = under;
    this.overlayMesh.visible = under;
  }

  private overlayMesh!: Mesh;

  private initOverlay(): void {
    const g = new BufferGeometry();
    g.setAttribute(
      'position',
      new BufferAttribute(new Float32Array([-1, -1, 0, 3, -1, 0, -1, 3, 0]), 3),
    );
    const m = new Mesh(g, this.water.underwater);
    m.frustumCulled = false;
    m.visible = false;
    m.matrixAutoUpdate = false;
    this.overlayScene.add(m);
    this.overlayMesh = m;
  }

  /** Cosmetic 60 Hz steps: the water frames and the cloud scroll. */
  tick(steps: number): void {
    this.water.tick(steps);
  }

  /** `true` while the camera is under water (probe, Plan/41 10.3). */
  get underwater(): boolean {
    return this.underwaterOn;
  }

  get hasStarted(): boolean {
    return this.started;
  }

  /** Re-registers the accounting after a context restore (the cache reset it). */
  onContextRestored(): void {
    for (const e of this.lands.values()) {
      this.deps.accounting.set(`land:${String(e.x)},${String(e.z)}`, e.bytes);
    }
  }

  /** The resident lands (probe, tests). */
  residentLands(): LandKey[] {
    return [...this.lands.values()].map((e) => ({ x: e.x, z: e.z }));
  }

  /** Patch level of one patch (tests, probe). */
  levelOf(x: number, z: number, px: number, pz: number): PatchLevel | undefined {
    return this.lands.get(this.key(x, z))?.levels.at(px, pz);
  }

  /** The index buffer of land `(x, z)` (tests). */
  meshOf(x: number, z: number): LandMesh | null {
    return this.lands.get(this.key(x, z))?.mesh ?? null;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.forced?.release();
    this.forced = null;
    this.streamer.dispose();
    for (const e of [...this.lands.values()]) {
      this.lands.delete(e.key);
      this.deps.accounting.remove(`land:${String(e.x)},${String(e.z)}`);
      this.disposeEntity(e);
    }
    this.field.clear();
    this.scene.clear();
    this.waterScene.clear();
    this.overlayMesh.geometry.dispose();
    this.overlayScene.clear();
    this.water.dispose();
  }
}
