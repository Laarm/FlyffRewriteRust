// The renderer facade (Plan/41 section 2.2, WP-REND-01): canvas lifecycle, the pass framework, the camera, the
// environment uniforms, the texture and model caches, the upload queue and the stats; the API consumed by doc 40's
// `GameLoop`, doc 43 and the feature handlers. Pieces owned by later work packages (world, picking, previews,
// overlays, SFX) are stubs with the final signatures.

import { Scene, type Object3D } from 'three';
import type { FlyffDebugProbe } from '../debug/probe';
import type { RenderApi } from '../game/ports';
import type { ClientFeatureConfig } from '../gen/config/clientFeatures';
import { RenderCamera } from './camera';
import { RenderDevice, type DeviceOptions } from './device';
import { StubRenderFx, type RenderFx, type SfxHandle } from './fx';
import { PARTS_LWEAPON, PARTS_RWEAPON } from '../gen/defs/neuz';
import { XI_GEN_MOVEMARK01 } from '../gen/defs/obj';
import { seededRandf } from './sfx/rng';
import { SfxPrototypes, type SfxSource } from './sfx/SfxPrototypes';
import { Ctrls } from './objects/ctrls';
// ---- housing / guild house (ui-house-quiz)
import { DeployGhost, GHOST_BLEND, GHOST_BLEND_RED } from './objects/deployGhost';
import type { DecorSink } from './objects/decor';
import { GroundItems, type GroundSource } from './objects/groundItems';
import { COMMON_SFX } from './sfx/common';
import { asProjectileView, asWaterMovers } from './sfx/views';
import { WaterFx } from './sfx/waterFx';
import { SfxEffects } from './sfx/Effects';
// ---- render-audio-m7 (WP-REND-27)
import { EventBalloons } from './movers/eventBalloon';
import { Papering, type PaperingRequest } from './objects/papering';
import { BossFx } from './sfx/bossFx';
import { BoneSource } from './sfx/bonePos';
import { SfxMeshes } from './sfx/SfxMeshes';
import { SfxRenderer } from './sfx/SfxRenderer';
import { SfxSystem, type SfxCatalog } from './sfx/SfxSystem';
import { SfxTextures } from './sfx/SfxTextures';
import type { SkillFxHooks } from './sfx/classSpec';
import type { YoyoHooks } from './sfx/yoyo';
import { FrameWorld } from './sfx/worldQuery';
import { BlobShadows } from './shadows/BlobShadows';
import { HighlightTwins } from './movers/HighlightTwins';
import { MoverBatcher, type MoverSimSink } from './movers/MoverBatcher';
import { MoverInstancer } from './movers/MoverInstancer';
import type { ModelProvider, VisualRegistryLike } from './movers/MoverVisual';
import { SimPoseSource } from './movers/poseSource';
import { MoverPickSource, PickSupport } from './pick/PickSupport';
import type { RenderViewViews } from '../wasm/simTypes';
import { DEFAULT_ENV, type EnvLight } from './gl/materials/env';
import { MaterialFactory } from './gl/materials/MaterialFactory';
import {
  TextureCache,
  TextureNotFoundError,
  type TextureHandle,
  type TextureLoader,
} from './gl/textures';
import { GpuAccounting, UploadQueue } from './gl/uploadQueue';
import { ModelCache } from './objects/ModelCache';
import { ModelInstance, type ModelInstanceOptions } from './objects/ModelInstance';
import { PassPipeline, scenePass } from './passes';
import type { SkyProbe } from './sky/Sky';
import { DEFAULT_RENDER_OPTIONS, type RenderOptions } from './quality';
import {
  renderCounters,
  StatsCollector,
  type ProbeRenderCounters,
  type RenderStats,
} from './stats';
import type {
  CameraState,
  EntityViewSource,
  FrameInput,
  GameClockSample,
  GaugeState,
  ObjId,
  PickProxy,
  Ray3,
  RenderEvent,
  RenderViewReaderLike,
  ScreenRect,
  TargetState,
  WorldRenderDesc,
} from './types';
import type { ModelGeometry } from './objects/ModelCache';
import type { PreviewProbeRow, PreviewRenderer } from './preview/PreviewRenderer';
import { MergedGeometryCache } from './movers/merge';
import type { MergeHost } from './movers/MoverVisual';
import type { PartElement } from './movers/partRules';
import type { RideAssets } from './movers/rides';
import { TerrainTexturePool } from './terrain/terrainTexturePool';
import type { WorldAssets } from './world/ports';
import { WorldScene, type StaticSfxSink, type WorldGate } from './world/WorldScene';

/** What the world's movers need from the game (WP-REND-12; `app/renderHost.ts` supplies it). */
export interface MoverAssets {
  /** Items, movers and models of the client bundles (`VisualRegistry`). */
  readonly registry: VisualRegistryLike;
  /** `.o3d` and `.chr` loading (the asset manager in the game). */
  readonly models: ModelProvider;
  /** `SET_VISIBLE` of the sim. */
  readonly sim: MoverSimSink;
  /** The look of a mover changed: its `SET_PARAMS` (bounds, stance, ride) must be sent again. */
  readonly onAppearanceChanged?: (id: number) => void;
  /**
   * The sim side of rides and wings (WP-REND-25): the attached decor entry of a ride (its sounds, a wing's bone palette), the wing's
   * skeleton and clips. Without it a ride is a static model and a wing stands in its bind pose.
   */
  readonly rides?: RideAssets;
  /** Random source of the eye blink and the hit shake in `[0, 1)` (default `Math.random`; the specs seed it). */
  readonly rand?: () => number;
  /**
   * `SimBridge.rayCast` for the ground height of dropped items (LH; a hit, `null`, or `undefined` when the bridge's query budget is
   * used up). Without it the items use the land height only.
   */
  readonly rayCast?: GroundSource['rayCast'];
  /**
   * The decor entries of the sim (WP-REND-23): `SET_VISIBLE` of the ctrls, the node clips of ctrl models, `DECOR_*` of animated static
   * objects. Without it ctrls and animated statics stand in their bind pose.
   */
  readonly decor?: DecorSink;
  /** `CtrlProp.dwSfxCtrl` of a ctrl id (the effect `SetActionPlay` creates); `undefined` / `NULL_ID`: none. */
  readonly ctrlSfx?: (ctrlId: number) => number | undefined;
  /** Called when the movers are detached (the provider releases its asset handles). */
  readonly dispose?: () => void;
}

/** The effect requests the renderer implements itself (the rest is counted by the base stub). */
class RendererFx extends StubRenderFx {
  constructor(
    private readonly shake: (id: number) => void,
    private readonly sfxSystem: () => SfxSystem | null,
    private readonly effects: () => SfxEffects | null,
    private readonly items: () => GroundItems | null,
    private readonly ctrls: () => Ctrls | null,
    private readonly boss: () => BossFx | null,
    private readonly balloons: () => EventBalloons | null,
    private readonly papering: () => Papering | null,
    private readonly papered: (req: PaperingRequest) => void,
  ) {
    super();
  }
  // ---- render-audio-m7 (WP-REND-27)
  bossMeleeHit(attacker: number, target: number, state: number, hitCount: number): void {
    this.boss()?.meleeHit(attacker, target, state, hitCount);
  }
  bossCharge(id: number, state: number): void {
    this.boss()?.charge(id, state);
  }
  eventBalloon(playerId: number, itemId: number | null): void {
    this.balloons()?.set(playerId, itemId);
  }
  roomPapering(o: PaperingRequest): void {
    this.papered(o);
    this.papering()?.apply(o);
  }
  override hitShake(id?: number): void {
    super.hitShake();
    if (id !== undefined) this.shake(id);
  }
  override sfx(xi?: number, o?: Parameters<RenderFx['sfx']>[1]): SfxHandle {
    const stub = super.sfx();
    const sys = this.sfxSystem();
    // handles of the stub (no system yet) are negative: they never collide with the system's
    return sys && xi !== undefined ? sys.create(xi, o ?? {}) : -stub;
  }
  override projectileVisual(
    id?: number,
    xi?: number,
    kind?: number,
    target?: number,
    attacker?: number,
  ): void {
    super.projectileVisual();
    if (id !== undefined && xi !== undefined && kind !== undefined) {
      this.effects()?.projectiles.visual(id, xi, kind, target ?? 0, attacker ?? 0);
    }
  }
  override projectileEnd(
    id?: number,
    at?: Parameters<RenderFx['projectileEnd']>[1],
    hitSfx?: number,
  ): void {
    super.projectileEnd();
    if (id !== undefined) this.effects()?.projectiles.end(id, at ?? null, hitSfx ?? 0);
  }
  meleeSparks(attacker: number, option: number): void {
    this.effects()?.sparks.hit(attacker, option);
  }
  skillYoyo(src: number, target: number, sfxId: number, hands: readonly (0 | 1)[]): void {
    this.effects()?.yoyo.launch(src, target, sfxId, hands);
  }
  hotAir(o: Parameters<NonNullable<RenderFx['hotAir']>>[0]): void {
    this.effects()?.hotAir.start(o);
  }
  itemDropped(id: number): void {
    this.items()?.drop(id);
  }
  ctrlPlace(id: number, kind: number): void {
    this.ctrls()?.commonPlace(id, kind);
  }
  override removeSfx(h?: SfxHandle): void {
    super.removeSfx();
    if (h !== undefined && h > 0) this.sfxSystem()?.remove(h);
  }
  override markRemoveSfx(anchor?: number, xi?: number): void {
    super.markRemoveSfx();
    if (anchor !== undefined && xi !== undefined) this.sfxSystem()?.markRemove(anchor, xi);
  }
  override moveMark(
    pos?: readonly [number, number, number],
    normal?: readonly [number, number, number] | null,
  ): void {
    super.moveMark();
    if (pos) this.sfxSystem()?.placeMoveMark(XI_GEN_MOVEMARK01, pos, normal ?? null);
  }
}

/** What the SFX runtime needs from the game (WP-REND-18; `app/renderHost.ts` supplies it). */
export interface SfxAssets {
  /** `.sfx` descriptions (the asset manager's `sfx` decode job). */
  readonly source: SfxSource;
  /** The `sfx-table` rows (`XI_` id -> file, scale, distance class, render flag). */
  readonly catalog: SfxCatalog;
  /** The models of the MESH parts (`model/<name>.o3d`); without it MESH parts draw nothing. */
  readonly models?: Pick<ModelProvider, 'model'>;
  /** Called when the SFX runtime is detached (the source releases its handles). */
  readonly dispose?: () => void;
}

/** A texture loader that has no assets (every texture is "missing" and renders white). */
export const NO_TEXTURES: TextureLoader = {
  load: (req) => Promise.reject(new TextureNotFoundError(req.path)),
};

/**
 * The public API of Plan/41 section 2.2 as consumed by doc 40's `GameLoop`, doc 43 and the feature handlers.
 * Consumers depend on THIS interface; the pieces owned by later work packages are stubs in the class.
 */
export interface WorldRendererApi {
  init(canvas: HTMLCanvasElement, opts: RenderOptions, feat: ClientFeatureConfig): Promise<void>;
  /** `null` leaves the world (Plan/41 section 22.5). */
  setWorld(w: WorldRenderDesc | null): void;
  setOptions(opts: RenderOptions): void;
  /** Cosmetic 60 Hz steps executed this iteration (0..60, R4). */
  tick(steps: number): void;
  draw(frame: FrameInput): void;
  onEvent(ev: RenderEvent): void;
  readonly fx: RenderFx;
  /** Picking support for doc 43 section 5 (WP-REND-15). */
  visibleObjects(): readonly PickProxy[];
  raycastPrecise(id: ObjId, rayThree: Ray3): number | null;
  projectBounds(id: ObjId): ScreenRect | null;
  /** `0` clears the highlight. */
  setHighlight(id: ObjId): void;
  setTargets(t: TargetState): void;
  setGauges(g: GaugeState): void;
  stats(): RenderStats;
  dispose(): void;
}

export interface WorldRendererOptions extends DeviceOptions {
  /** Where texture bytes come from (the asset manager + worker pool in the game, a fetch loader in dev pages). */
  readonly textureLoader?: TextureLoader;
  /** `draw()` runs `uploads.runFrame()` first (standalone pages; the game loop does it itself, 40 section 10.2). */
  readonly autoUploads?: boolean;
  /** `gl.getError()` after texture uploads (tests). */
  readonly checkErrors?: boolean;
  /** Called once per missing/failed texture path. */
  readonly onMissingTexture?: (path: string, reason: string) => void;
  /** Diagnostics of the world part (a land failed, a layer was removed, a static model is missing). */
  readonly onWorldLog?: (message: string) => void;
  /**
   * Wraps the renderer's `fx` (the base counts the requests and drives the hit shake): the game host composes the in-world
   * overlay in so that `renderer.fx.balloon(...)` reaches the Canvas2D layer (`overlay/OverlayRenderer.composeFx`).
   */
  readonly fxWrap?: (base: RenderFx) => RenderFx;
  /** `[0, 1)` source of the weather particles (default `Math.random`; the specs seed it). */
  readonly rand?: () => number;
}

/** The renderer of doc 41. */
export class WorldRenderer implements WorldRendererApi {
  readonly passes = new PassPipeline();
  /** The shared scene of the model queues. */
  readonly scene = new Scene();
  readonly cameraRig = new RenderCamera();
  readonly uploads = new UploadQueue();
  readonly accounting = new GpuAccounting();
  readonly models = new ModelCache(this.accounting);
  readonly collector = new StatsCollector();
  /** Merged mover geometry shared by movers with the same parts (WP-REND-05 part merging). */
  readonly mergedGeometry = new MergedGeometryCache();
  /** The effect requests (`RendererFx`, wrapped by `WorldRendererOptions.fxWrap` when the host composes the overlay in). */
  readonly fx: RenderFx;
  private deviceRef: RenderDevice | null = null;
  private texturesRef: TextureCache | null = null;
  private materialsRef: MaterialFactory | null = null;
  private envState: EnvLight = DEFAULT_ENV;
  private optionsState: RenderOptions = { ...DEFAULT_RENDER_OPTIONS };
  private world: WorldRenderDesc | null = null;
  private cosmeticSteps = 0;
  private disposed = false;
  private readonly instances = new Set<ModelInstance>();
  private night = false;
  private features: ClientFeatureConfig | null = null;
  private textureLoader: TextureLoader;
  private previewRenderer: PreviewRenderer | null = null;
  /** The terrain texture pool (Plan/41 9.4); created by `init`. */
  private terrainPoolRef: TerrainTexturePool | null = null;
  private worldAssets: WorldAssets | null = null;
  private worldSceneRef: WorldScene | null = null;
  // the world's movers (WP-REND-12), their blob shadows (13), the pick support (15) and the highlight (12.9)
  private moversRef: MoverBatcher | null = null;
  private poseRef: SimPoseSource | null = null;
  private shadowsRef: BlobShadows | null = null;
  private instancerRef: MoverInstancer | null = null;
  private pickRef: PickSupport | null = null;
  private moverAssets: MoverAssets | null = null;
  private readonly highlight = new HighlightTwins();
  // the SFX runtime (WP-REND-18)
  private sfxRef: SfxSystem | null = null;
  private sfxGl: SfxRenderer | null = null;
  private sfxMeshesRef: SfxMeshes | null = null;
  private sfxEffectsRef: SfxEffects | null = null;
  // ---- render-audio-m7 (WP-REND-27): the boss effects
  private bossRef: BossFx | null = null;
  private balloonsRef: EventBalloons | null = null;
  /** The room papering (wall paper on the statics, carpet on the terrain) and the last request the game made (re-applied to a rebuilt world of the same room). */
  private readonly paperingRef: Papering = new Papering(() => {
    const w = this.worldSceneRef;
    if (!w) return null;
    return {
      setTextureOverride: (file, tex) => w.statics.setTextureOverride(file, tex),
      clearTextureOverrides: () => {
        w.statics.clearTextureOverrides();
      },
      forceTexture: (path) => {
        w.terrain.forceTexture(path);
      },
    };
  });
  private paperingReq: PaperingRequest | null = null;
  private itemsRef: GroundItems | null = null;
  private ctrlsRef: Ctrls | null = null;
  // ---- housing / guild house (ui-house-quiz)
  private deployRef: DeployGhost | null = null;
  private waterRef: WaterFx | null = null;
  /** The render view and the entity table of the last drawn frame (the 60 Hz steps of the water read them). */
  private lastView: RenderViewReaderLike | null = null;
  private lastEntities: EntityViewSource | null = null;
  private sfxTexturesRef: SfxTextures | null = null;
  private sfxProtosRef: SfxPrototypes | null = null;
  private sfxAssets: SfxAssets | null = null;
  private readonly sfxWorld = new FrameWorld();
  /** The game's answers for the skill effect classes (`setSkillHooks`). */
  private skillHooks: SkillFxHooks = {};
  private targets: TargetState | null = null;
  private gauges: GaugeState | null = null;

  constructor(private readonly config: WorldRendererOptions = {}) {
    const fx = new RendererFx(
      (id) => this.moversRef?.hitShake(id),
      () => this.sfxRef,
      () => this.sfxEffectsRef,
      () => this.itemsRef,
      () => this.ctrlsRef,
      () => this.bossRef,
      () => this.balloonsRef,
      () => this.paperingRef,
      (req) => {
        this.paperingReq = req;
      },
    );
    this.fx = config.fxWrap ? config.fxWrap(fx) : fx;
    this.textureLoader = config.textureLoader ?? NO_TEXTURES;
    // world matrices are computed where an object is placed (`ModelInstance.setTransform`), not by a traversal of
    // the whole graph in every frame
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
    this.passes.register(scenePass('opaque', 'opaque', true));
    this.passes.register(scenePass('blended', 'blended', true));
    this.passes.register(scenePass('translucent', 'translucent', true));
    // pass 5 (mouse-over highlight) and pass 8 (blob shadows); empty layers cost no draw call
    this.passes.register(scenePass('highlight', 'highlight', false));
    this.passes.register(scenePass('blobShadows', 'shadow', false));
  }

  // ---- accessors ----------------------------------------------------------------------------------------

  get device(): RenderDevice {
    if (!this.deviceRef) throw new Error('WorldRenderer.init() has not run');
    return this.deviceRef;
  }

  get textures(): TextureCache {
    if (!this.texturesRef) throw new Error('WorldRenderer.init() has not run');
    return this.texturesRef;
  }

  get materials(): MaterialFactory {
    if (!this.materialsRef) throw new Error('WorldRenderer.init() has not run');
    return this.materialsRef;
  }

  get options(): RenderOptions {
    return this.optionsState;
  }

  get environment(): EnvLight {
    return this.envState;
  }

  get initialised(): boolean {
    return this.deviceRef !== null;
  }

  /** `dispose()` ran (the probe's `render` section skips a disposed renderer). */
  get isDisposed(): boolean {
    return this.disposed;
  }

  get clientFeatures(): ClientFeatureConfig | null {
    return this.features;
  }

  // ---- lifecycle ----------------------------------------------------------------------------------------

  /** Creates the device and the caches on `canvas`. Throws `RenderUnsupportedError` without WebGL2. */
  init(canvas: HTMLCanvasElement, opts: RenderOptions, feat: ClientFeatureConfig): Promise<void> {
    if (this.deviceRef) return Promise.reject(new Error('WorldRenderer.init() ran twice'));
    this.optionsState = { ...opts };
    this.features = feat;
    const device = new RenderDevice(canvas, { ...this.config, options: this.optionsState });
    this.deviceRef = device;
    this.collector.stats.tier = opts.tier;
    const textures = new TextureCache({
      renderer: device.textureHost,
      caps: device.caps,
      loader: { load: (req) => this.textureLoader.load(req) },
      uploads: this.uploads,
      accounting: this.accounting,
      options: () => ({
        fullMips: this.optionsState.fullMips,
        anisotropy: this.optionsState.anisotropy,
      }),
      ...(this.config.onMissingTexture ? { onMissing: this.config.onMissingTexture } : {}),
      ...(this.config.checkErrors !== undefined ? { checkErrors: this.config.checkErrors } : {}),
    });
    this.texturesRef = textures;
    this.materialsRef = new MaterialFactory({
      white: () => textures.whiteTexture(),
      neutralReflect: () => textures.solidTexture([64, 64, 64, 255]),
    });
    this.materialsRef.setEnvironment(this.envState);
    const pool = new TerrainTexturePool({
      renderer: device.textureHost,
      caps: device.caps,
      loader: { load: (req) => this.textureLoader.load(req) },
      uploads: this.uploads,
      accounting: this.accounting,
      ...(this.config.onMissingTexture ? { onMissing: this.config.onMissingTexture } : {}),
      ...(this.config.checkErrors !== undefined ? { checkErrors: this.config.checkErrors } : {}),
    });
    this.terrainPoolRef = pool;
    device.onContextLost(() => {
      textures.onContextLost();
      pool.onContextLost();
      this.poseRef?.onContextLost();
      this.collector.stats.contextLost = device.contextLost;
    });
    device.onContextRestored(() => {
      textures.onContextRestored();
      this.models.onContextRestored();
      this.poseRef?.onContextRestored();
      pool.onContextRestored();
      this.worldSceneRef?.onContextRestored();
      this.collector.stats.contextRestored = device.contextRestored;
    });
    this.rebuildWorld();
    device.resize(
      canvas.clientWidth || canvas.width || 1,
      canvas.clientHeight || canvas.height || 1,
    );
    return Promise.resolve();
  }

  /** Replaces the source of texture data (the asset manager becomes available, dev pages). */
  setTextureLoader(loader: TextureLoader): void {
    this.textureLoader = loader;
  }

  /** The canvas CSS size changed. */
  resize(cssWidth: number, cssHeight: number): void {
    this.device.resize(cssWidth, cssHeight);
  }

  /** `RenderOptions` changed (options store). */
  setOptions(opts: RenderOptions): void {
    this.optionsState = { ...opts };
    this.collector.stats.tier = opts.tier;
    this.device.setOptions(this.optionsState);
  }

  /**
   * The world to show (`null` leaves the world, Plan/41 section 22.5): drops everything of the previous world (land meshes,
   * static instances, handles) and, once the asset ports are known (`setWorldAssets`), starts the environment, the
   * terrain streaming and the static objects of the new one.
   */
  setWorld(w: WorldRenderDesc | null): void {
    this.world = w;
    // the sim of the old world is gone: every mover (and what the sim was told about it) starts over
    this.moversRef?.reset();
    this.highlight.clear();
    this.sfxRef?.clear();
    this.bossRef?.clear();
    this.sfxEffectsRef?.clear();
    this.itemsRef?.clear();
    this.ctrlsRef?.clear();
    this.balloonsRef?.clear();
    this.waterRef?.clear();
    this.rebuildWorld();
  }

  /** The asset ports of the world renderer (lands, models, the client registry); doc 40's manager, or a dev page. */
  setWorldAssets(assets: WorldAssets | null): void {
    this.worldAssets = assets;
    this.rebuildWorld();
  }

  private rebuildWorld(): void {
    this.worldSceneRef?.dispose();
    this.worldSceneRef = null;
    const w = this.world;
    const assets = this.worldAssets;
    const pool = this.terrainPoolRef;
    this.paperingRef.forget();
    if (!w || !assets || !pool || !this.deviceRef || !this.texturesRef || !this.materialsRef)
      return;
    this.worldSceneRef = new WorldScene({
      device: this.deviceRef,
      scene: this.scene,
      passes: this.passes,
      cameraRig: this.cameraRig,
      materials: this.materialsRef,
      textures: this.texturesRef,
      models: this.models,
      uploads: this.uploads,
      accounting: this.accounting,
      pool,
      assets,
      options: () => this.optionsState,
      desc: w,
      // rain splashes (`XI_GEN_RAINCIRCLE01`) go through the effect API, overlay composition included
      fx: () => this.fx,
      staticSfx: () => this.staticSfxSink,
      // the animated static objects are decor entries of the sim (WP-REND-23; the movers may attach after the world)
      decor: () => this.moverAssets?.decor,
      ...(this.config.rand ? { rand: this.config.rand } : {}),
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
    // the overrides of the old world died with it (`OutProcessing`); the same room again gets its paper back
    this.paperingRef.reapply(w.worldId, this.paperingReq);
  }

  /** The world part (terrain, statics, environment tracker), `null` until a world and its asset ports are set. */
  get worldScene(): WorldScene | null {
    return this.worldSceneRef;
  }

  /** The render side of the loading gate (Plan/40 9.8): terrain ring and own-land static models. */
  worldGate(): WorldGate | null {
    return this.worldSceneRef?.gate() ?? null;
  }

  /** The terrain texture pool (probe, tests). */
  get terrainPool(): TerrainTexturePool {
    if (!this.terrainPoolRef) throw new Error('WorldRenderer.init() has not run');
    return this.terrainPoolRef;
  }

  get currentWorld(): WorldRenderDesc | null {
    return this.world;
  }

  /** The light and fog of the current frame (WP-REND-07 computes it from the game clock). */
  setEnvironment(env: EnvLight): void {
    this.envState = env;
  }

  // ---- per frame ----------------------------------------------------------------------------------------

  /** Cosmetic 60 Hz steps executed this iteration (0..60, R4). */
  tick(steps: number): void {
    const n = Math.max(0, Math.trunc(steps));
    this.cosmeticSteps += n;
    if (n > 0) {
      this.worldSceneRef?.tick(n);
      this.moversRef?.tick(n);
      this.itemsRef?.tick(n);
      this.ctrlsRef?.tick(n);
      this.balloonsRef?.tick(n);
      this.waterRef?.tick(
        n,
        asWaterMovers(this.lastView),
        this.worldSceneRef?.terrain.field ?? null,
      );
      // the boss continue logic runs before the effects' own step (render-audio-m7)
      this.bossRef?.tick(n);
      this.sfxRef?.tick(n);
      this.sfxEffectsRef?.tick(n);
    }
  }

  get cosmeticStepCount(): number {
    return this.cosmeticSteps;
  }

  /** Runs the upload queue (the game loop calls its own queue; standalone pages call this). */
  runUploads(): void {
    this.uploads.runFrame();
    this.collector.stats.uploadBytes = this.uploads.last.bytes;
  }

  /**
   * Runs the upload queue until `p` settles (standalone pages and tests: the textures of a model resolve only when
   * their upload job ran). Rejects after `timeoutMs`.
   */
  async pump(p: Promise<unknown>, timeoutMs = 10000): Promise<void> {
    const flag = { done: false };
    const finish = (): void => {
      flag.done = true;
    };
    void p.then(finish, finish);
    const t0 = performance.now();
    while (!flag.done) {
      this.runUploads();
      await new Promise<void>((r) => setTimeout(r, 0));
      if (performance.now() - t0 > timeoutMs)
        throw new Error('timeout while waiting for GPU uploads');
    }
  }

  /** Draws one frame. */
  draw(frame: FrameInput): void {
    if (this.disposed) return;
    const device = this.device;
    if (device.isLost) return;
    const t0 = performance.now();
    if (this.config.autoUploads === true) this.runUploads();
    this.cameraRig.update(frame.camera, device.aspect);
    // the world computes the light of the frame from the game clock (WP-REND-07); without a world the last
    // `setEnvironment` stays in force (dev pages)
    const worldEnv = this.worldSceneRef?.update(frame) ?? null;
    if (worldEnv) this.envState = worldEnv;
    this.materials.setEnvironment(this.envState);
    if (this.envState.night !== this.night) {
      this.night = this.envState.night;
      for (const i of this.instances) i.setNight(this.night);
    }
    this.updateMovers(frame);
    this.lastView = frame.view;
    this.lastEntities = frame.entities;
    this.itemsRef?.update(frame.entities, frame.camera.pos, this.cameraRig.frustum);
    this.ctrlsRef?.update(frame.entities, frame.view, frame.camera.pos, this.cameraRig.frustum);
    this.balloonsRef?.update(frame.nowMs);
    this.updateSfx(frame);
    device.beginFrame(this.envState.fogColor);
    const ran = this.passes.run({
      device,
      camera: this.cameraRig,
      scene: this.scene,
      frame,
      env: this.envState,
      options: this.optionsState,
    });
    device.present();
    const s = this.collector;
    s.setDrawCalls({ ...ran.calls, ramp: 1 }, ran.triangles);
    s.stats.frames++;
    s.stats.vramMb = this.accounting.totalMb;
    s.stats.shaderPermutations = this.materials.permutations;
    s.stats.programs = device.renderer.info.programs?.length ?? 0;
    s.stats.textures = device.renderer.info.memory.textures;
    s.stats.missingTextures = this.textures.missing + (this.terrainPoolRef?.missing ?? 0);
    s.stats.landsResident = this.worldSceneRef?.terrain.landsResident ?? 0;
    s.stats.visibleMovers = this.moversRef?.stats.drawn ?? 0;
    s.stats.paletteMatrices = this.poseRef?.paletteMatrices ?? 0;
    s.stats.paletteBytes = this.poseRef?.paletteBytes ?? 0;
    s.stats.visibleStatics = this.worldSceneRef?.statics.visibleInstances ?? 0;
    s.stats.sfxInstances = this.sfxRef?.stats.instances ?? 0;
    s.stats.sfxQuads = this.sfxRef?.stats.quads ?? 0;
    s.stats.sfxDropped = this.sfxRef?.stats.dropped ?? 0;
    s.stats.gpuMs = device.gpuTimer?.last ?? null;
    s.recordCpuMs(performance.now() - t0);
  }

  /** Routed `SimWorld` events (cosmetic kinds are handled by later WPs). */
  onEvent(): void {
    // the cosmetic kinds are handled by WP-REND-12/18/21/...
  }

  // ---- model convenience --------------------------------------------------------------------------------

  /**
   * Loads the sphere map of `XE_REFLECT` blocks (`model/texture/etc_reflect.tga`, served as PNG, ref/18 section 8.2)
   * and installs it in the material system; until it settled reflect blocks use a neutral map. Resolves with the
   * texture handle (kept for the life of the renderer), `failed` when the file does not exist.
   */
  async loadReflectMap(path = 'model/texture/etc_reflect.tga'): Promise<TextureHandle> {
    const h = this.textures.acquire(path, 'world');
    await this.pump(h.ready);
    if (h.state === 'ready') this.materials.setReflectTexture(h.texture);
    return h;
  }

  /**
   * A drawable instance of a resident model, added to the shared scene (or to `parent`: the root group of a mover
   * visual, a preview scene).
   */
  createInstance(
    model: ModelGeometry,
    options?: ModelInstanceOptions,
    parent: Object3D = this.scene,
  ): ModelInstance {
    const inst = new ModelInstance(
      model,
      { materials: this.materials, textures: this.textures },
      options,
    );
    inst.setNight(this.night);
    parent.add(inst.root);
    this.instances.add(inst);
    return inst;
  }

  removeInstance(inst: ModelInstance): void {
    this.instances.delete(inst);
    inst.dispose();
  }

  /** What the merged bodies of mover visuals need (`MoverVisual`'s `VisualHost.merge`). */
  get merge(): MergeHost {
    return { materials: this.materials, textures: this.textures, cache: this.mergedGeometry };
  }

  // ---- previews (WP-REND-06) ------------------------------------------------------------------------------

  /** Registers the preview renderer (built by the game boot / the preview fixture page) for `preview()` and the probe. */
  attachPreviews(p: PreviewRenderer | null): void {
    this.previewRenderer = p;
  }

  /** `renderer.preview()` of Plan/41 section 2.2. */
  preview(): PreviewRenderer {
    if (!this.previewRenderer) throw new Error('no PreviewRenderer is attached (attachPreviews)');
    return this.previewRenderer;
  }

  /** `__flyffDebug.render.previews`: `{ slot, preset, motion, nonEmptyPixels }[]`. */
  previewProbe(): readonly PreviewProbeRow[] {
    return this.previewRenderer?.probe() ?? [];
  }

  // ---- movers (WP-REND-12), blob shadows (13), picking (15) -----------------------------------------------

  /**
   * Gives the renderer what it needs to draw the world's movers (`null` detaches them). Without it `draw` renders terrain and
   * statics only. The `SimWorld` render view arrives with every `FrameInput.view`; the palette texture is created here, on the
   * renderer's GL context.
   */
  attachMovers(assets: MoverAssets | null): void {
    this.detachMovers();
    this.moverAssets = assets;
    if (!assets || !this.deviceRef || !this.texturesRef || !this.materialsRef) return;
    const device = this.deviceRef;
    const pose = SimPoseSource.followingView(device.textureHost);
    this.poseRef = pose;
    const instancer = new MoverInstancer({
      scene: this.scene,
      materials: this.materialsRef,
      palette: pose.palette,
    });
    this.instancerRef = instancer;
    const batcher = new MoverBatcher({
      host: this,
      parent: this.scene,
      pose,
      models: assets.models,
      registry: assets.registry,
      sim: assets.sim,
      instancer,
      ...(assets.rand ? { rand: assets.rand } : {}),
      ...(assets.onAppearanceChanged ? { onAppearanceChanged: assets.onAppearanceChanged } : {}),
      ...(assets.rides ? { rides: assets.rides } : {}),
      options: () => this.optionsState,
      bindView: (view) => {
        const v = view as (RenderViewViews & { valid?: boolean }) | null;
        if (!v || v.valid === false) return false;
        pose.useView(v);
        return true;
      },
    });
    this.moversRef = batcher;
    // ---- render-audio-m7: the bones and ranks of the movers for the boss effects (`WorldQuery.bonePos`, `rankOf`)
    this.sfxWorld.bones = new BoneSource({
      read: (id, out) => pose.read(id, out),
      skeletonOf: (id) => batcher.visualOf(id)?.skeleton ?? null,
    });
    this.sfxWorld.rank = (id) => batcher.entry(id)?.rank;
    this.shadowsRef = new BlobShadows({
      scene: this.scene,
      materials: this.materialsRef,
      textures: this.texturesRef,
      models: assets.models,
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
    const pick = new PickSupport(
      this.cameraRig,
      () => ({ width: device.cssWidth, height: device.cssHeight }),
      () => batcher.frameNumber,
    );
    pick.register(new MoverPickSource({ batcher, pose }));
    this.pickRef = pick;
    // the ground items (WP-REND-22): models by item id, the drop bounce, the spin, the shine
    const items = new GroundItems({
      reflectOf: (id) => assets.registry.item(id)?.reflect ?? 0,
      modelOf: (id) => assets.registry.itemModel?.(id),
      models: assets.models,
      host: {
        createInstance: (model, options, parent) => this.createInstance(model, options, parent),
        removeInstance: (inst) => {
          if (inst instanceof ModelInstance) this.removeInstance(inst);
        },
      },
      ground: {
        rayCast: (o, d, t, m) => assets.rayCast?.(o, d, t, m),
        heightAt: (x, z) => this.worldSceneRef?.terrain.field.heightAt(x, z) ?? 0,
      },
      sfx: (xi, o) => {
        this.sfxRef?.create(xi, o);
      },
      objectDistant: () => this.optionsState.objectDistant,
      ...(assets.rand ? { rnd: assets.rand } : {}),
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
    this.itemsRef = items;
    pick.register(items.pickSource());
    // the controls (WP-REND-23): chests, doors, traps; the node animation comes from the decor area of the render view
    const ctrls = new Ctrls({
      modelOf: (id) => assets.registry.ctrlModel?.(id),
      sfxOf: (id) => assets.ctrlSfx?.(id),
      models: assets.models,
      host: {
        createInstance: (model, options, parent) => this.createInstance(model, options, parent),
        removeInstance: (inst) => {
          if (inst instanceof ModelInstance) this.removeInstance(inst);
        },
      },
      ...(assets.decor ? { decor: assets.decor } : {}),
      sfx: (xi, o) => {
        this.sfxRef?.create(xi, o);
      },
      objectDistant: () => this.optionsState.objectDistant,
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
    this.ctrlsRef = ctrls;
    pick.register(ctrls.pickSource());
    // ---- render-audio-m7 (WP-REND-27): the event balloons beside the players (the `OT_ITEM` model of the buff's item)
    this.balloonsRef = new EventBalloons({
      posOf: (id, out) => {
        const v = this.lastView as unknown as {
          valid?: boolean;
          posOf?: (id: number, out: number[]) => boolean;
        } | null;
        return v?.valid !== false && v?.posOf?.(id, out) === true;
      },
      isDrawn: (id) => batcher.entry(id)?.drawn === true,
      speedOf: (id) => this.skillHooks.speedOf?.(id),
      modelOf: (id) => assets.registry.itemModel?.(id),
      models: assets.models,
      host: {
        createInstance: (model, options, parent) => this.createInstance(model, options, parent),
        removeInstance: (inst) => {
          if (inst instanceof ModelInstance) this.removeInstance(inst);
        },
      },
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
    // ---- housing / guild house (ui-house-quiz): the placement ghost (ONE model, not pickable, no shadow) and the geometry of its collision test
    this.deployRef = new DeployGhost({
      modelOf: (id) => assets.registry.ctrlModel?.(id),
      models: assets.models,
      host: {
        createInstance: (model, options, parent) => this.createInstance(model, options, parent),
        removeInstance: (inst) => {
          if (inst instanceof ModelInstance) this.removeInstance(inst);
        },
      },
      staticInstances: (file) => this.worldSceneRef?.statics.instancesOf(file) ?? [],
      ctrlBoxes: () => this.ctrlsRef?.worldBoxes() ?? [],
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
  }

  private detachMovers(): void {
    this.sfxWorld.bones = null;
    this.sfxWorld.rank = null;
    this.highlight.clear();
    this.shadowsRef?.dispose();
    this.shadowsRef = null;
    this.moversRef?.dispose();
    this.moversRef = null;
    this.instancerRef?.dispose();
    this.instancerRef = null;
    this.poseRef?.dispose();
    this.poseRef = null;
    this.pickRef = null;
    this.itemsRef?.dispose();
    this.itemsRef = null;
    this.ctrlsRef?.dispose();
    this.ctrlsRef = null;
    this.balloonsRef?.dispose();
    this.balloonsRef = null;
    // ---- housing / guild house (ui-house-quiz)
    this.deployRef?.dispose();
    this.deployRef = null;
    this.moverAssets?.dispose?.();
    this.moverAssets = null;
  }

  /** The mover batcher (probe, tests), `null` until `attachMovers`. */
  get movers(): MoverBatcher | null {
    return this.moversRef;
  }

  /** The ctrls (WP-REND-23: probe, tests), `null` until `attachMovers`. */
  get ctrls(): Ctrls | null {
    return this.ctrlsRef;
  }

  // ---- housing / guild house (ui-house-quiz)
  /** The placement ghost (`DeployRenderApi` of the game, WP-CFEAT-46), `null` until `attachMovers`. */
  get deploy(): DeployGhost | null {
    return this.deployRef;
  }

  /** The mover instancer (probe, tests; `enabled` switches the batching off), `null` until `attachMovers`. */
  get instancer(): MoverInstancer | null {
    return this.instancerRef;
  }

  /** The blob shadows (probe, tests), `null` until `attachMovers`. */
  get shadows(): BlobShadows | null {
    return this.shadowsRef;
  }

  /** Picking support (`visibleObjects`, `raycastPrecise`, `projectBounds`) for sources that register (ground items, ctrls). */
  get pick(): PickSupport | null {
    return this.pickRef;
  }

  /** The sky, weather and lens flare state of the last frame (`__flyffDebug.render.sky`), `null` between worlds. */
  skyProbe(): SkyProbe | null {
    return this.worldSceneRef?.sky.probe() ?? null;
  }

  /** The loading gate of the own mover (Plan/40 9.8): `true` while no movers are attached (nothing to wait for). */
  ownModelReady(id: number): boolean {
    return this.moversRef ? this.moversRef.ownModelReady(id) : true;
  }

  /** The `render.ctrls` probe: the stats and one row per ctrl (load state, drawn, pick box), plus the proxies the pick support sees. */
  ctrlsProbe(): Record<string, unknown> | null {
    const c = this.ctrlsRef;
    if (!c) return null;
    const proxies = this.pickRef?.visibleObjects().filter((p) => p.kind === 'ctrl').length ?? 0;
    return { ...c.probe(), proxies };
  }

  /**
   * The `render.statics` probe (BFIX1 integration): the statics counters of the last frame, the groups whose instanced draw is
   * capped below its instance count (`[]` is the pass condition of #015/#018), the bounding-sphere facts, and the animated
   * (decor) objects with the LOD they show (#022: all `0`). `null` between worlds.
   */
  staticsProbe(): Record<string, unknown> | null {
    const st = this.worldSceneRef?.statics;
    if (!st) return null;
    const animated = st.instanceProbe().filter((e) => e.animated);
    return {
      ...st.stats(),
      instanceLimitViolations: st.instanceLimitViolations(),
      spheres: st.sphereProbe(),
      animated: {
        instances: animated.length,
        drawn: animated.filter((e) => e.drawn).length,
        lods: [...new Set(animated.map((e) => e.lod))].sort((a, b) => a - b),
      },
    };
  }

  /** The `render.movers` probe rows. */
  moversProbe(): Record<string, unknown> | null {
    const m = this.moversRef;
    if (!m) return null;
    return {
      ...m.probe(),
      instancing: this.instancerRef ? { ...this.instancerRef.stats } : null,
      // the instanced groups (mover batches, blob shadows) whose draw is capped below their instance count: `[]` is the pass condition
      instanceLimitViolations: [
        ...(this.instancerRef?.instanceLimitViolations() ?? []),
        ...(this.shadowsRef?.instanceLimitViolations() ?? []),
      ],
      shadows: this.shadowsRef?.count ?? 0,
      shadowSkips: this.shadowsRef ? { ...this.shadowsRef.skipped } : null,
      proxies: this.pickRef?.visibleObjects().length ?? 0,
      highlight: this.highlight.count,
    };
  }

  /** Mover visuals, shadows and the highlight of this frame (called by `draw` before the passes). */
  private updateMovers(frame: FrameInput): void {
    const movers = this.moversRef;
    if (!movers) return;
    movers.update(frame, this.cameraRig.frustum);
    const shadows = this.shadowsRef;
    if (shadows) {
      shadows.enabled = this.optionsState.shadow === 2;
      shadows.update(movers.drawn, this.worldSceneRef?.terrain.field ?? null);
    }
    // pass 5: the highlighted mover (never a TRANSPARENT_MODE one) gets its additive twins
    const hi = movers.highlighted;
    const e = hi !== 0 ? movers.entry(hi) : undefined;
    this.highlight.set(e?.drawn && e.decorated ? e.visual : null);
    this.highlight.sync();
  }

  // ---- the SFX runtime (WP-REND-18) --------------------------------------------------------------------------------

  /**
   * Gives the renderer what it needs to draw effects (`null` detaches them): the `.sfx` source, the `sfx-table` rows. The effect
   * requests of `renderer.fx` made before this call are dropped (cosmetic); the pass 12 draws them from then on.
   */
  attachSfx(assets: SfxAssets | null): void {
    this.detachSfx();
    this.sfxAssets = assets;
    if (!assets || !this.deviceRef || !this.texturesRef) return;
    const textures = new SfxTextures(this.texturesRef);
    const scene = new Scene();
    const meshes = assets.models
      ? new SfxMeshes({
          models: assets.models,
          parent: scene,
          host: {
            createInstance: (model, options, parent) => this.createInstance(model, options, parent),
            removeInstance: (inst) => {
              if (inst instanceof ModelInstance) this.removeInstance(inst);
            },
          },
          ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
        })
      : null;
    const gl = new SfxRenderer({
      textureOf: (id) => textures.textureOf(id),
      scene,
      ...(meshes ? { meshes } : {}),
    });
    const protos = new SfxPrototypes(assets.source, this.config.onWorldLog);
    const system = new SfxSystem({
      prototypes: protos,
      textures,
      catalog: assets.catalog,
      renderer: gl,
      ...(meshes ? { meshes } : {}),
      options: () => this.optionsState,
      // the laser and the fire breath of the bosses, once per frame behind the instances (render-audio-m7)
      extras: {
        draw: (list, frame, tex) => {
          this.bossRef?.draw(list, frame, tex);
        },
      },
      // the specs seed the generator through `rand`: the CRT generator itself (RANDF <= 0.6553, KEEP) stays
      ...(this.config.rand
        ? { randf: seededRandf(Math.floor(this.config.rand() * 0x1_0000_0000)) }
        : {}),
      ...(this.config.onWorldLog ? { log: this.config.onWorldLog } : {}),
    });
    this.sfxTexturesRef = textures;
    this.sfxProtosRef = protos;
    this.sfxGl = gl;
    this.sfxMeshesRef = meshes;
    this.sfxRef = system;
    this.passes.register(gl.pass());
    system.hooks = this.skillHooks;
    this.sfxWorld.height = (x, z) => this.worldSceneRef?.terrain.field.heightAt(x, z);
    this.bossRef = new BossFx({
      create: (xi, o) => system.create(xi, o),
      remove: (h) => {
        system.remove(h);
      },
      instanceOf: (h) => system.instanceOf(h),
      world: () => this.sfxWorld,
      propOf: (id) => (this.lastEntities?.get(id) as { propId?: number } | undefined)?.propId,
      hooks: () => this.skillHooks,
      better: () => this.features?.features.better_clockwork_alpha ?? true,
      ...(this.config.onWorldLog ? { warn: this.config.onWorldLog } : {}),
    });
    const effects = new SfxEffects({
      sys: system,
      textures: this.texturesRef,
      world: () => this.sfxWorld,
      sfxRenderOff: () => this.optionsState.sfxRenderOff,
      created: (xi, pos) => this.skillHooks.sfxCreated?.(xi, pos),
      yoyoHooks: () => this.yoyoHooks(),
      hotAirPainTick: () => this.skillHooks.hotAirPainTick?.bind(this.skillHooks),
      ...(this.config.rand ? { rnd: this.config.rand } : {}),
    });
    this.sfxEffectsRef = effects;
    this.waterRef = new WaterFx({
      sfx: (xi, o) => {
        system.create(xi, o);
      },
      modeOf: (id) => {
        const rec = this.lastEntities?.get(id) as { visual?: { mode?: number } } | undefined;
        return rec?.visual?.mode ?? 0;
      },
    });
    effects.preload();
    // the first spark of a fight, a level-up or a click on the ground should not wait for a download
    system.preload(COMMON_SFX);
    for (const p of effects.passes()) this.passes.register(p);
    // static `MODELTYPE_SFX` objects of the lands that are already resident would be missed: the world is attached first in
    // the game, so they arrive through the sink below
  }

  private detachSfx(): void {
    if (this.sfxGl) this.passes.unregister('sfx');
    this.waterRef = null;
    if (this.sfxEffectsRef) {
      this.passes.unregister('ribbons');
      this.passes.unregister('particles');
    }
    this.sfxEffectsRef?.dispose();
    this.sfxEffectsRef = null;
    this.bossRef?.clear();
    this.bossRef = null;
    this.sfxRef?.dispose();
    this.sfxMeshesRef?.dispose();
    this.sfxGl?.dispose();
    this.sfxTexturesRef?.dispose();
    this.sfxProtosRef?.clear();
    this.sfxAssets?.dispose?.();
    this.sfxRef = null;
    this.sfxGl = null;
    this.sfxMeshesRef = null;
    this.sfxTexturesRef = null;
    this.sfxProtosRef = null;
    this.sfxAssets = null;
  }

  /**
   * The game's answers for the skill effect classes (party skill effects, duel state, the Hot Air audio cue; WP-REND-24). May be
   * called before or after `attachSfx`.
   */
  setSkillHooks(hooks: SkillFxHooks): void {
    this.skillHooks = hooks;
    if (this.sfxRef) this.sfxRef.hooks = hooks;
  }

  /** The weapon element of a mover's hand (the yo-yo flights hide it and copy its model: `__YOYO_MODEL_FIX`). */
  private weaponElement(src: number, hand: 0 | 1): PartElement | undefined {
    const plan = this.moversRef?.visualOf(src)?.currentPlan;
    const slot = hand === 0 ? PARTS_RWEAPON : PARTS_LWEAPON;
    return plan?.elements.find((e) => e.slot === slot);
  }

  private yoyoHooks(): YoyoHooks {
    return {
      weaponModel: (src, hand) => this.weaponElement(src, hand)?.file ?? null,
      hideWeapon: (src, hand, hidden) => {
        const el = this.weaponElement(src, hand);
        if (el) el.hidden = hidden;
      },
    };
  }

  /** The SFX runtime (probe, tests, the combat and projectile hooks), `null` until `attachSfx`. */
  get sfx(): SfxSystem | null {
    return this.sfxRef;
  }

  /** The static-effect consumer of the lands (`MODELTYPE_SFX` placements). */
  private get staticSfxSink(): StaticSfxSink | undefined {
    const sys = this.sfxRef;
    if (!sys) return undefined;
    return {
      landReady: (key, placements, registry) => {
        sys.addStaticLand(
          key,
          placements.flatMap((p) => {
            const info = registry.objectModel(p.index);
            if (!info?.file) return [];
            return [
              {
                file: info.file,
                distant: info.distant,
                renderFlag: info.renderFlag,
                pos: p.pos,
                angle: p.angle,
                scale: p.scale,
              },
            ];
          }),
        );
      },
      landEvicted: (key) => {
        sys.removeStaticLand(key);
      },
    };
  }

  private updateSfx(frame: FrameInput): void {
    const sys = this.sfxRef;
    if (!sys) return;
    const rig = this.cameraRig;
    this.sfxEffectsRef?.update({
      view: asProjectileView(frame.view),
      cameraPos: frame.camera.pos,
      pointScale: 2 / rig.camera.projectionMatrix.elements[5],
      viewportHeight: this.device.bufferHeight,
    });
    sys.update({
      cameraPos: frame.camera.pos,
      frustum: rig.frustum,
      invViewRot: rig.invViewRotLH,
      world: this.sfxWorld.set(frame),
    });
  }

  /** The `render.balloons()` probe (render-audio-m7): one row per event balloon (player, item, position, bob, load state, drawn); `null` until `attachMovers`. */
  balloonsProbe(): Record<string, unknown> | null {
    const b = this.balloonsRef;
    return b ? { count: b.count, created: b.created, rows: b.probe() } : null;
  }

  /**
   * The `render.ghost()` probe (render-audio-m7, WP-REND-27): the draw state of the placement ghost - `{ctrlId, loaded, visible, blend, red, pos, angle}`, `blend` 80 free and 110 while it collides,
   * `red` = the `red.tga` alternates are on (exactly when the collision flag is set); `null` without a ghost.
   */
  ghostProbe(): Record<string, unknown> | null {
    const g = this.deployRef?.probe() ?? null;
    if (!g) return null;
    return { ...g, blend: g.collide ? GHOST_BLEND_RED : GHOST_BLEND, red: g.collide };
  }

  /** The `render.papering()` probe (render-audio-m7): the wall and floor of the current room and how many wall groups were rebound. */
  paperingProbe(): Record<string, unknown> {
    const t = this.worldSceneRef?.terrain.forcedPath ?? null;
    return { ...this.paperingRef.probe(), terrainForced: t };
  }

  /** The `render.boss()` probe (render-audio-m7): charges, laser ticks, sparks, breath particles, missing poses; `null` until `attachSfx`. */
  bossProbe(): Record<string, unknown> | null {
    return this.bossRef ? { ...this.bossRef.probe() } : null;
  }

  /** The `render.sfx` probe rows. */
  sfxProbe(): Record<string, unknown> | null {
    const s = this.sfxRef;
    if (!s) return null;
    return {
      ...s.stats,
      textures: this.sfxTexturesRef?.count ?? 0,
      texturesFailed: this.sfxTexturesRef?.failed ?? 0,
      texturesLoading: this.sfxTexturesRef?.loading ?? 0,
      prototypes: this.sfxProtosRef?.size ?? 0,
      failures: this.sfxProtosRef ? [...this.sfxProtosRef.failures.keys()].slice(0, 20) : [],
      glSlots: this.sfxGl?.slots ?? null,
      meshes: this.sfxMeshesRef?.stats ?? null,
      meshesLoading: this.sfxMeshesRef?.stats.loading ?? 0,
      effects: this.sfxEffectsRef?.probe() ?? null,
    };
  }

  visibleObjects(): readonly PickProxy[] {
    return this.pickRef?.visibleObjects() ?? [];
  }
  raycastPrecise(id: ObjId, ray: Ray3): number | null {
    return this.pickRef?.raycastPrecise(id, ray) ?? null;
  }
  projectBounds(id: ObjId): ScreenRect | null {
    return this.pickRef?.projectBounds(id) ?? null;
  }
  setHighlight(id: ObjId): void {
    this.moversRef?.setHighlight(id);
  }

  // ---- stubs of later work packages ---------------------------------------------------------------------

  /** The brackets of the next frame (WP-REND-20; drawn by the overlay layer, which reads {@link targetState}). */
  setTargets(t: TargetState): void {
    this.targets = t;
  }
  /** The own player's world gauges (the charge gauge of a bow or wand); drawn by the overlay layer ({@link gaugeState}). */
  setGauges(g: GaugeState): void {
    this.gauges = g;
  }
  /** The brackets last set (`null` before the first `setTargets`). */
  get targetState(): TargetState | null {
    return this.targets;
  }
  get gaugeState(): GaugeState | null {
    return this.gauges;
  }

  /** A copy of the current stats (also at `window.__flyffDebug.render`). */
  stats(): RenderStats {
    return this.collector.snapshot();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.detachSfx();
    this.detachMovers();
    this.worldSceneRef?.dispose();
    this.worldSceneRef = null;
    for (const i of [...this.instances]) this.removeInstance(i);
    this.models.dispose();
    this.mergedGeometry.dispose();
    this.terrainPoolRef?.dispose();
    this.texturesRef?.dispose();
    this.deviceRef?.dispose();
  }
}

/** What the loop adapter needs besides the frame itself (doc 40 section 10.2 step 10). */
export interface RenderPortSources {
  /** The game clock sample of this frame (doc 40 `environment.sample()`). */
  gameClock(): GameClockSample;
  /** The entity table (`ObjId` to record), `null` before it exists. */
  entities(): EntityViewSource | null;
}

/**
 * The loop-facing port of `game/ports.ts` over a renderer: `tick(due)` forwards the cosmetic steps, `draw` maps the
 * loop's `RenderFrame` (opaque `view` and `camera`) to the renderer's `FrameInput`.
 */
export function toRenderApi(renderer: WorldRendererApi, sources: RenderPortSources): RenderApi {
  return {
    tick: (due) => {
      renderer.tick(due);
    },
    draw: (frame) => {
      renderer.draw({
        view: frame.view as RenderViewReaderLike | null,
        camera: frame.camera as CameraState,
        gameClock: sources.gameClock(),
        nowMs: frame.nowMs,
        entities: sources.entities(),
      });
    },
  };
}

/** The renderers whose stats a probe's `render` section answers (the latest live one first in line, see `installRenderProbe`). */
const probeRenderers = new WeakMap<object, WorldRenderer[]>();
/** Renderers that only host previews (the core windows' paper doll and portrait in the world): listed in `previews` only. */
const probePreviewHosts = new WeakMap<object, WorldRenderer[]>();

/**
 * Lists the previews of `renderer` in `render.previews` WITHOUT making it the renderer the stats read (the preview service of
 * the in-game windows runs a second renderer beside the world renderer; WP-CFEAT-11).
 */
export function addPreviewProbeRenderer(probe: FlyffDebugProbe, renderer: WorldRenderer): void {
  const list = probePreviewHosts.get(probe) ?? [];
  if (!list.includes(renderer)) list.push(renderer);
  probePreviewHosts.set(probe, list);
}

/**
 * Attaches the `render` section of the debug probe (`window.__flyffDebug.render`, Plan/41 section 24): every
 * `RenderStats` field as a getter plus `stats()` (a full copy); the section itself is callable, `render()` returns the five
 * counters of `ProbeRenderCounters` (Plan/60 13.2, WP-REND-17). The game has two renderers over its life - the preview
 * renderer of the title screens and the world renderer - and ONE section: a later call with ANOTHER renderer of the same
 * probe retargets the stats to the newest renderer that is not disposed, and `previews` lists the previews of all of them.
 * The same renderer twice is an error (the section name is unique).
 */
export function installRenderProbe(probe: FlyffDebugProbe, renderer: WorldRenderer): void {
  const known = probeRenderers.get(probe);
  if (known) {
    if (known.includes(renderer)) throw new Error('probe section "render" already exists');
    known.push(renderer);
    return;
  }
  const renderers: WorldRenderer[] = [renderer];
  probeRenderers.set(probe, renderers);
  const current = (): WorldRenderer => {
    for (let i = renderers.length - 1; i >= 0; i--) {
      const r = renderers[i];
      if (r && !r.isDisposed) return r;
    }
    return renderers[renderers.length - 1] ?? renderer;
  };
  // the section is CALLABLE: `render()` answers the five counters of the last frame; the properties below stay on the function
  // object (no `RenderStats` key is `name` or `length`)
  const section = Object.assign((): ProbeRenderCounters => renderCounters(current().stats()), {
    stats: () => current().stats(),
    options: () => ({ ...current().options }),
    caps: () => (current().initialised ? { ...current().device.caps } : null),
  });
  for (const key of Object.keys(renderer.stats())) {
    Object.defineProperty(section, key, {
      enumerable: true,
      get: () => (current().stats() as unknown as Record<string, unknown>)[key],
    });
  }
  // `render.movers`: the mover batcher of the newest renderer (entities, visuals, drawn, SET_VISIBLE counts, one row per mover)
  Object.defineProperty(section, 'movers', {
    enumerable: true,
    get: () => current().moversProbe(),
  });
  // `render.statics`: the static objects of the world (counters, instance-limit violations, spheres, animated LODs)
  Object.defineProperty(section, 'statics', {
    enumerable: true,
    get: () => current().staticsProbe(),
  });
  // `render.ctrls`: the ctrls of the world (load state, drawn flag, pick box, pick proxies)
  Object.defineProperty(section, 'ctrls', {
    enumerable: true,
    get: () => current().ctrlsProbe(),
  });
  // `render.sfx`: the effect runtime (instances, drawn, quads, runs, rejected requests, textures, prototypes)
  Object.defineProperty(section, 'sfx', {
    enumerable: true,
    get: () => current().sfxProbe(),
  });
  // `render.ghost`: the placement ghost (ui-house-quiz's, verified by render-audio-m7); callable: `render.ghost()`
  Object.defineProperty(section, 'ghost', {
    enumerable: true,
    get: () => () => current().ghostProbe(),
  });
  // `render.papering`: the wall paper and carpet of the room (render-audio-m7, WP-REND-27); callable: `render.papering()`
  Object.defineProperty(section, 'papering', {
    enumerable: true,
    get: () => () => current().paperingProbe(),
  });
  // `render.balloons`: the event balloons beside the players (render-audio-m7, WP-REND-27); callable: `render.balloons()`
  Object.defineProperty(section, 'balloons', {
    enumerable: true,
    get: () => () => current().balloonsProbe(),
  });
  // `render.boss`: the boss effects (render-audio-m7, WP-REND-27); callable: `render.boss()`
  Object.defineProperty(section, 'boss', {
    enumerable: true,
    get: () => () => current().bossProbe(),
  });
  // `render.sky`: the layers, the sun / moon, the flare and the weather of the last frame (WP-REND-28/29)
  Object.defineProperty(section, 'sky', {
    enumerable: true,
    get: () => current().skyProbe(),
  });
  // `render.previews[i].nonEmptyPixels` (Plan/41 section 21.3)
  Object.defineProperty(section, 'previews', {
    enumerable: true,
    get: () =>
      [...renderers, ...(probePreviewHosts.get(probe) ?? [])]
        .filter((r) => !r.isDisposed)
        .flatMap((r) => r.previewProbe()),
  });
  probe.extend('render', section);
}
