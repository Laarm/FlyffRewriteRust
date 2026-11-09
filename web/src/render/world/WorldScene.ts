// The world part of the renderer (Plan/41 sections 8-11, 22; WP-REND-07/08/09/10/11/16): the environment, the terrain with its
// streaming, the water and cloud sea, the static objects, the passes they draw in and the loading gate. One instance per
// world; `WorldRenderer.setWorld(desc)` creates it, `setWorld(null)` / a new world disposes it (Plan/41 22.5: every
// land mesh, instance buffer and handle of the world is released, the shared caches stay under their LRU).
//
// Per drawn frame the renderer calls `update(frame)`, which returns the light of the frame; per cosmetic step batch
// `tick(steps)`.

import type { Scene } from 'three';
import { Prio } from '../../workers/priority';
import type { RenderCamera } from '../camera';
import type { RenderDevice } from '../device';
import { EnvironmentTracker, toEnvLight, type EnvironmentState } from '../env/environment';
import type { EnvLight } from '../gl/materials/env';
import type { MaterialFactory } from '../gl/materials/MaterialFactory';
import type { TextureCache } from '../gl/textures';
import type { GpuAccounting, UploadQueue } from '../gl/uploadQueue';
import type { DecorSink } from '../objects/decor';
import type { ModelCache } from '../objects/ModelCache';
import { StaticObjects } from '../objects/StaticObjects';
import type { PassPipeline } from '../passes';
import type { RenderOptions } from '../quality';
import type { RenderFx } from '../fx';
import { createOcclusion } from '../sky/occlusion';
import { SkySystem } from '../sky/Sky';
import { landOf } from '../terrain/streaming';
import { Terrain } from '../terrain/Terrain';
import type { TerrainTexturePool } from '../terrain/terrainTexturePool';
import type { FrameInput, Vec3LH, WorldRenderDesc } from '../types';
import type { WorldAssets, WorldRegistry } from './ports';

/** Idle time after which a zero-reference GPU resource is freed (Plan/40 9.3). */
export const GPU_IDLE_MS = 120_000;
/** How often the residency policy runs. */
export const TRIM_INTERVAL_MS = 1000;

/** The consumer of the `MODELTYPE_SFX` placements of a land: the SFX runtime (WP-REND-18; static effects loop forever). */
export interface StaticSfxSink {
  landReady(
    key: number,
    placements: readonly { index: number; pos: Vec3LH; angle: number; scale: Vec3LH }[],
    registry: WorldRegistry,
  ): void;
  landEvicted(key: number): void;
}

export interface WorldSceneDeps {
  readonly device: RenderDevice;
  /** The shared model scene (layers of the passes 3, 4, 13): the static objects live here. */
  readonly scene: Scene;
  readonly passes: PassPipeline;
  readonly cameraRig: RenderCamera;
  readonly materials: MaterialFactory;
  readonly textures: TextureCache;
  readonly models: ModelCache;
  readonly uploads: UploadQueue;
  readonly accounting: GpuAccounting;
  readonly pool: TerrainTexturePool;
  readonly assets: WorldAssets;
  readonly options: () => RenderOptions;
  readonly desc: WorldRenderDesc;
  /** The renderer's effect API (rain splashes of the weather); may be absent. */
  readonly fx?: () => RenderFx | undefined;
  /** Where the static SFX placements of the lands go (resolved at each call: the SFX runtime may attach after the world). */
  readonly staticSfx?: () => StaticSfxSink | undefined;
  /** The sim side of animated static objects (resolved at each use: the movers attach after the world); absent = bind pose. */
  readonly decor?: () => DecorSink | undefined;
  /** `[0, 1)` source of the weather particles (seeded in the specs). */
  readonly rand?: () => number;
  readonly log?: (message: string) => void;
}

/** What the loading gate of doc 40 (Plan/40 9.8 step 5) waits for on the render side. */
export interface WorldGate {
  /** The inner 3 x 3 present lands are resident (terrain). */
  readonly terrain: { readonly ready: number; readonly wanted: number };
  /** The static models of the viewer's own land are loaded. */
  readonly statics: { readonly ready: number; readonly wanted: number };
  /** `true` when every part is complete. */
  readonly open: boolean;
  /** Weighted completion 0..1 (terrain 80 %, own-land statics 20 %; doc 40 weights the whole gate). */
  readonly progress: number;
}

export class WorldScene {
  readonly env = new EnvironmentTracker();
  readonly terrain: Terrain;
  readonly statics: StaticObjects;
  /** The sky, the weather and the lens flare (passes 1, 14, 15; indoor worlds draw no sky). */
  readonly sky: SkySystem;
  /** The environment of the last frame (probe). */
  last: EnvironmentState | null = null;
  private disposed = false;
  private own: { x: number; z: number } | null = null;
  private lastTrim = 0;
  /** Objects freed by the residency policy so far (probe, tests). */
  trimmed = { models: 0, textures: 0 };

  constructor(private readonly deps: WorldSceneDeps) {
    const d = deps.desc;
    this.env.setWorld(d.folder, d.settings, d.continents);
    this.statics = new StaticObjects({
      scene: deps.scene,
      materials: deps.materials,
      textures: deps.textures,
      models: deps.models,
      source: deps.assets.models,
      registry: deps.assets.registry,
      uploads: deps.uploads,
      accounting: deps.accounting,
      options: deps.options,
      indoor: d.indoor,
      mpu: d.mpu,
      ...(deps.decor ? { decor: deps.decor } : {}),
      ...(deps.log ? { log: deps.log } : {}),
    });
    this.terrain = new Terrain({
      renderer: deps.device.renderer,
      pool: deps.pool,
      textures: deps.textures,
      uploads: deps.uploads,
      accounting: deps.accounting,
      env: deps.materials.env,
      assets: deps.assets,
      options: deps.options,
      world: d,
      onLandReady: (land) => {
        const own = this.own;
        const isOwn = own !== null && own.x === land.x && own.z === land.z;
        const near = own !== null && Math.abs(own.x - land.x) <= 1 && Math.abs(own.z - land.z) <= 1;
        this.statics.addLand(land.x, land.z, land.objects, d.landsW, {
          prio: isOwn ? Prio.Critical : near ? Prio.Near : Prio.Statics,
          distance: own ? Math.hypot(own.x - land.x, own.z - land.z) : 0,
        });
        deps
          .staticSfx?.()
          ?.landReady(
            land.x + land.z * d.landsW,
            this.statics.sfxPlacements(land.x, land.z, d.landsW),
            deps.assets.registry,
          );
      },
      onLandEvicted: (x, z) => {
        this.statics.removeLand(x, z, d.landsW);
        deps.staticSfx?.()?.landEvicted(x + z * d.landsW);
      },
      ...(deps.log ? { log: deps.log } : {}),
    });
    this.sky = new SkySystem({
      textures: deps.textures,
      desc: d,
      options: deps.options,
      ground: this.terrain.field,
      occlusion: () => createOcclusion(this.terrain.field, this.statics),
      ...(deps.fx ? { fx: deps.fx } : {}),
      ...(deps.rand ? { rand: deps.rand } : {}),
    });
    for (const pass of this.sky.passes) deps.passes.register(pass);
    deps.passes.register({
      name: 'terrain',
      enabled: true,
      render: (ctx) => {
        ctx.device.renderLayer(this.terrain.scene, ctx.camera.camera, null, true);
      },
    });
    deps.passes.register({
      name: 'water',
      enabled: true,
      render: (ctx) => {
        ctx.device.renderLayer(this.terrain.waterScene, ctx.camera.camera, null, true);
      },
    });
    deps.passes.register({
      name: 'underwater',
      enabled: true,
      render: (ctx) => {
        if (this.terrain.underwater) {
          ctx.device.renderLayer(this.terrain.overlayScene, ctx.camera.camera, null, false);
        }
      },
    });
  }

  /**
   * One drawn frame: the environment for the game clock, the streaming around the active mover, the drawing around the
   * camera, the static objects. Returns the light of the frame (`null` while the environment cannot be computed).
   */
  update(frame: FrameInput): EnvLight | null {
    if (this.disposed) return null;
    const d = this.deps.desc;
    // the server weather (stores `worldStore.env`) before the environment samples the state that carries it
    if (frame.weather) this.sky.weatherInput(frame.weather.season, frame.weather.effect);
    this.env.setWeather(this.sky.weather.kind, this.sky.weather.controller.locked);
    const state = this.env.sample(frame.gameClock);
    this.last = state;
    const cam = frame.camera.pos;
    const active: Vec3LH = frame.activePos ?? frame.camera.lookAt;
    const land = landOf(active[0], active[2], d.mpu);
    this.own = {
      x: Math.min(d.landsW - 1, Math.max(0, land.x)),
      z: Math.min(d.landsH - 1, Math.max(0, land.z)),
    };
    this.terrain.update(cam, active, frame.nowMs);
    this.statics.setViewer(active, this.own);
    if (state) {
      this.statics.setNight(state.night);
      this.sky.update({
        layers: state.sky,
        sunAngle: state.sunAngle,
        hour: state.hour,
        min: state.min,
        sec: state.sec,
        camera: { pos: frame.camera.pos, lookAt: frame.camera.lookAt },
        active,
        cameraWorld: this.deps.cameraRig.camera.matrixWorld.elements,
      });
    }
    this.statics.update(this.deps.cameraRig);
    // the node poses of the animated objects (the clocks of the sim run whether or not the camera moved)
    this.statics.updateDecor(frame.view);
    if (frame.nowMs - this.lastTrim >= TRIM_INTERVAL_MS) {
      this.lastTrim = frame.nowMs;
      this.trim(frame.nowMs);
    }
    return state ? toEnvLight(state) : null;
  }

  /**
   * The residency policy (Plan/41 22.3, Plan/40 9.3): zero-reference models and textures that idle for longer than
   * `GPU_IDLE_MS` are freed; when the resident GPU bytes exceed the tier budget (`vramBudgetMb`) the least recently used
   * zero-reference ones go first until the budget holds. Referenced resources are never evicted (`stats.vramOver` is
   * the caller's concern when they alone exceed the budget).
   */
  trim(nowMs: number): void {
    const budget = this.deps.options().vramBudgetMb * 1024 * 1024;
    const over = this.deps.accounting.totalBytes > budget;
    this.trimmed.models += this.deps.models.trim(over ? 0 : GPU_IDLE_MS, nowMs);
    this.trimmed.textures += this.deps.textures.trim(budget, GPU_IDLE_MS, nowMs);
  }

  /** Cosmetic 60 Hz steps (water frames, cloud scroll, occluder fades). */
  tick(steps: number): void {
    if (this.disposed) return;
    this.terrain.tick(steps);
    this.statics.tick(steps, this.deps.cameraRig);
    this.sky.tick(steps);
  }

  /** The render side of the loading gate (Plan/40 9.8). */
  gate(): WorldGate {
    const t = this.terrain.gate();
    const own = this.own;
    const s = own
      ? this.statics.landStatus(own.x, own.z, this.deps.desc.landsW)
      : { wanted: 0, ready: 0 };
    const terrainOk = t.open;
    const staticsOk = s.ready >= s.wanted;
    const sp = s.wanted === 0 ? 1 : s.ready / s.wanted;
    return {
      terrain: { ready: t.ready, wanted: t.wanted },
      statics: { ready: s.ready, wanted: s.wanted },
      open: terrainOk && staticsOk && this.terrain.hasStarted,
      progress: 0.8 * t.progress + 0.2 * sp,
    };
  }

  onContextRestored(): void {
    this.terrain.onContextRestored();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const pass of this.sky.passes) this.deps.passes.unregister(pass.name);
    this.deps.passes.unregister('terrain');
    this.deps.passes.unregister('water');
    this.deps.passes.unregister('underwater');
    this.statics.dispose();
    this.terrain.dispose();
    this.sky.dispose();
    this.env.setWorld('', null);
  }
}
