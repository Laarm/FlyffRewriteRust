// The sky renderer (Plan/41 section 14, WP-REND-28 / -29; SkyBox.cpp `Render` 278-745, pass 1; weather pass 14; lens flare pass 15).
// Outdoor worlds only. Every drawn frame the world scene passes the current `SkyFrame` to `update`; the three passes then draw:
//
//   pass 1   the sky cylinder (1 or 2 cross-fade layers), the cloud dome (same schedule, scrolling), the moon or the sun disk and
//            its glow, in the SKY VIEW (the camera's rotation, translation zero), depth test and write off, fog off. The clear
//            colour (the fog colour) shows through wherever the layers are transparent.
//   pass 14  rain / snow (`Weather.ts`)
//   pass 15  the lens flare (`LensFlare.ts`)
//
// Textures come from the texture cache (`weather/*.dds|tga|bmp`, `sky` sampler); a layer whose texture is not resident is not
// drawn (never the white stand-in of a failed texture). The cloud scroll `(u, v) += 0.0002` is per 60 Hz tick (FIX of the
// per-rendered-frame original).

import {
  BufferAttribute,
  BufferGeometry,
  Mesh,
  PerspectiveCamera,
  Scene,
  type Texture,
} from 'three';
import { Prio } from '../../workers/priority';
import type { Mat4 } from '../d3dx';
import { sunAngle as sunAngleOf } from '../env/sunMoon';
import type { RenderFx } from '../fx';
import type { TextureCache } from '../gl/textures';
import type { PassContext, RenderPass } from '../passes';
import type { RenderOptions } from '../quality';
import { mirrorMatrix, mirrorPositionsInPlace } from '../space';
import type { Vec3LH, WorldRenderDesc } from '../types';
import {
  billboardQuad,
  bodyOfHour,
  bodyPosition,
  GLOW_HALF,
  mirrorQuadInto,
  MOON_HALF,
  rotatedQuad,
  SUN_HALF,
  type Body,
} from './celestial';
import { buildCylinder, buildDome, QUAD_INDICES, QUAD_UVS, type SkyMeshData } from './geometry';
import { LensFlare, type SunOcclusion } from './LensFlare';
import { skySchedule, type SkyKind, type SkyLayers } from './schedule';
import {
  createBodyMaterial,
  createLayerMaterial,
  type BodyBlend,
  type BodyMaterial,
  type LayerMaterial,
} from './skyShaders';
import { layerPath, SkyTextures } from './skyTextures';
import { skyViewLH, viewCentre } from './skyView';
import { Weather } from './Weather';
import type { FallGround, WeatherKind } from './weatherSim';

/** What the sky needs of a frame (Plan/41 sections 8.3, 14). */
export interface SkyFrame {
  /** Layers of the cylinder and of the cloud dome (the same schedule). */
  readonly layers: SkyLayers;
  /** `m_fSunAngle` as the sky render leaves it (the renderer derives the angle of the quads from the clock). */
  readonly sunAngle: number;
  readonly hour: number;
  readonly min: number;
  readonly sec: number;
  /** The camera of the frame, LH. */
  readonly camera: { readonly pos: Vec3LH; readonly lookAt: Vec3LH };
  /** The active mover's position (where the weather particles spawn). */
  readonly active: Vec3LH;
  /** The three.js camera's `matrixWorld.elements` (the snow billboards face the camera). */
  readonly cameraWorld: ArrayLike<number>;
}

export interface SkyRenderer {
  /** `false` in indoor worlds. */
  enabled: boolean;
  update(frame: SkyFrame): void;
  /** Cosmetic 60 Hz steps (cloud scroll `(u, v) += 0.0002` per tick, the weather particles, the flare cache). */
  tick(steps: number): void;
  dispose(): void;
}

/** A do-nothing sky (tests of the world scene that do not need one). */
export class StubSky implements SkyRenderer {
  enabled = true;
  /** The last frame handed to `update` (tests and the probe read it). */
  last: SkyFrame | null = null;
  updates = 0;
  update(frame: SkyFrame): void {
    this.last = frame;
    this.updates++;
  }
  tick(): void {
    // nothing to scroll
  }
  dispose(): void {
    this.last = null;
  }
}

/** Per-tick step of the cloud scroll (u and v). */
export const CLOUD_STEP = 0.0002;

export interface SkyDeps {
  readonly textures: Pick<TextureCache, 'acquire'>;
  readonly desc: Pick<WorldRenderDesc, 'worldId' | 'indoor' | 'settings'>;
  readonly options: () => RenderOptions;
  /** The visual ground queries (rain, splashes): the terrain's height field. */
  readonly ground: FallGround;
  /** The sun occlusion queries (terrain and object boxes); `null` = never occluded. */
  readonly occlusion?: () => SunOcclusion | null;
  /** The renderer's effect API (rain splashes). */
  readonly fx?: () => RenderFx | undefined;
  /** `[0, 1)` source of the particles (seeded in the specs). */
  readonly rand?: () => number;
}

interface LayerSlot {
  readonly mesh: Mesh;
  readonly material: LayerMaterial;
}

interface BodySlot {
  readonly mesh: Mesh;
  readonly material: BodyMaterial;
  readonly position: BufferAttribute;
}

function layerGeometry(data: SkyMeshData): BufferGeometry {
  const g = new BufferGeometry();
  const positions = new Float32Array(data.positions);
  mirrorPositionsInPlace(positions);
  g.setAttribute('position', new BufferAttribute(positions, 3));
  g.setAttribute('uv', new BufferAttribute(data.uvs, 2));
  g.setAttribute('aAlpha', new BufferAttribute(data.alphas, 1));
  if (data.indices) g.setIndex(new BufferAttribute(data.indices, 1));
  return g;
}

/** What the probe and the specs read of the sky. */
export interface SkyProbe {
  readonly enabled: boolean;
  readonly layers: readonly {
    readonly kind: SkyKind;
    readonly weight: number;
    readonly drawn: boolean;
  }[];
  readonly clouds: readonly {
    readonly kind: SkyKind;
    readonly weight: number;
    readonly drawn: boolean;
  }[];
  readonly cloudOffset: readonly [number, number];
  readonly body: Body;
  readonly bodyDrawn: boolean;
  readonly glowDrawn: boolean;
  readonly flare: { readonly alpha: number; readonly drawn: boolean; readonly evaluations: number };
  readonly weather: {
    readonly kind: WeatherKind;
    readonly locked: boolean;
    readonly season: number;
    readonly drawing: boolean;
    readonly particles: number;
    readonly ticks: number;
    readonly splashes: number;
    /** Milliseconds per particle step (moving average). */
    readonly tickMs: number;
  };
  readonly textures: Readonly<Record<string, string>>;
  /** CPU milliseconds of `update` (the last frame and a moving average): the sky's share of the 2 ms scene-update budget. */
  readonly cpuMs: { readonly last: number; readonly avg: number };
}

export class SkySystem implements SkyRenderer {
  /** `false` hides the sky, the sun / moon and the flare (indoor worlds); the weather follows its own rules. */
  enabled: boolean;
  readonly textures: SkyTextures;
  readonly weather: Weather;
  readonly flare: LensFlare;
  /** The scene of pass 1. */
  readonly scene = new Scene();
  /** Pass 1, 14 and 15 (registered by the world scene). */
  readonly passes: readonly RenderPass[];
  private readonly camera = new PerspectiveCamera();
  private readonly cylinder: LayerSlot[] = [];
  private readonly dome: LayerSlot[] = [];
  private readonly moon: BodySlot;
  private readonly sun: BodySlot;
  private readonly glow: BodySlot;
  private readonly geometries: BufferGeometry[] = [];
  private readonly corners = new Float32Array(12);
  private readonly mirrored = new Float32Array(12);
  private cloudTicks = 0;
  private skyView: Mat4 | null = null;
  private hasLayers = false;
  private body: Body = 'none';
  private last: {
    layers: SkyProbe['layers'];
    clouds: SkyProbe['clouds'];
    bodyDrawn: boolean;
    glowDrawn: boolean;
  } = { layers: [], clouds: [], bodyDrawn: false, glowDrawn: false };
  private disposed = false;
  private updateMs = 0;
  private updateMsAvg = 0;

  constructor(deps: SkyDeps) {
    this.enabled = !deps.desc.indoor;
    this.textures = new SkyTextures(
      deps.textures,
      deps.desc.settings,
      () => deps.options().textureQuality,
    );
    this.camera.matrixAutoUpdate = false;
    this.camera.matrixWorldAutoUpdate = false;
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;

    const cyl = layerGeometry(buildCylinder());
    const dome = layerGeometry(buildDome());
    this.geometries.push(cyl, dome);
    const layer = (g: BufferGeometry, name: string): LayerSlot => {
      const material = createLayerMaterial(name);
      const mesh = new Mesh(g, material);
      mesh.frustumCulled = false;
      mesh.matrixAutoUpdate = false;
      mesh.visible = false;
      this.scene.add(mesh);
      return { mesh, material };
    };
    // draw order: the older layer of a cross-fade first
    this.cylinder.push(layer(cyl, 'sky-cylinder-0'), layer(cyl, 'sky-cylinder-1'));
    this.dome.push(layer(dome, 'sky-dome-0'), layer(dome, 'sky-dome-1'));
    const body = (name: string, blend: BodyBlend): BodySlot => {
      const g = new BufferGeometry();
      const position = new BufferAttribute(new Float32Array(12), 3);
      g.setAttribute('position', position);
      g.setAttribute('uv', new BufferAttribute(QUAD_UVS, 2));
      g.setIndex(new BufferAttribute(QUAD_INDICES, 1));
      this.geometries.push(g);
      const material = createBodyMaterial(name, blend);
      const mesh = new Mesh(g, material);
      mesh.frustumCulled = false;
      mesh.matrixAutoUpdate = false;
      mesh.visible = false;
      this.scene.add(mesh);
      return { mesh, material, position };
    };
    this.moon = body('sky-moon', 'srcAlphaOne');
    this.sun = body('sky-sun', 'add');
    this.glow = body('sky-glow', 'alpha');

    this.weather = new Weather({
      world: { worldId: deps.desc.worldId, indoor: deps.desc.indoor },
      options: deps.options,
      textures: this.textures,
      ground: deps.ground,
      ...(deps.fx ? { fx: deps.fx } : {}),
      ...(deps.rand ? { rand: deps.rand } : {}),
    });
    this.flare = new LensFlare(this.textures, deps.occlusion ?? (() => null));

    this.passes = [
      {
        name: 'sky',
        enabled: true,
        render: (ctx) => {
          if (!this.enabled || !this.hasLayers) return;
          ctx.device.renderLayer(this.scene, this.skyCamera(ctx), null, false);
        },
      },
      {
        name: 'weather',
        enabled: true,
        render: (ctx) => {
          this.weather.pass.render(ctx);
        },
      },
      {
        name: 'flare',
        enabled: true,
        render: (ctx) => {
          if (!this.enabled || !this.flare.drawing) return;
          ctx.device.renderLayer(this.flare.scene, this.skyCamera(ctx), null, false);
        },
      },
    ];
  }

  /** The sky camera: the world projection with the sky view. */
  private skyCamera(ctx: PassContext): PerspectiveCamera {
    const cam = this.camera;
    cam.projectionMatrix.copy(ctx.camera.camera.projectionMatrix);
    cam.projectionMatrixInverse.copy(ctx.camera.camera.projectionMatrixInverse);
    return cam;
  }

  /** The texture requests of a frame: the current layers soon, the next game hour's and the bodies in the background. */
  private request(frame: SkyFrame): void {
    const paths = this.textures.paths();
    for (const l of frame.layers.layers) {
      this.textures.request(layerPath(paths, 'sky', l.kind), Prio.Near);
      this.textures.request(layerPath(paths, 'cloud', l.kind), Prio.Near);
    }
    const next = skySchedule((frame.hour % 24) + 1, frame.min);
    for (const l of next.layers) {
      this.textures.request(layerPath(paths, 'sky', l.kind), Prio.Fx);
      this.textures.request(layerPath(paths, 'cloud', l.kind), Prio.Fx);
    }
    const body = bodyOfHour(frame.hour);
    if (body === 'sun') {
      this.textures.request(paths.sun, Prio.Near);
      this.textures.request(paths.glow, Prio.Near);
    } else if (body === 'moon') {
      this.textures.request(paths.moon, Prio.Near);
    }
    this.textures.request(paths.sun, Prio.Fx);
    this.textures.request(paths.moon, Prio.Fx);
    this.textures.request(paths.glow, Prio.Fx);
  }

  /** The cloud scroll offset `(u, v)` after the ticks so far (wrapped to 0..1). */
  cloudOffset(): [number, number] {
    const v = (this.cloudTicks * CLOUD_STEP) % 1;
    return [v, v];
  }

  update(frame: SkyFrame): void {
    if (this.disposed) return;
    const t0 = performance.now();
    this.updateFrame(frame);
    this.updateMs = performance.now() - t0;
    this.updateMsAvg += (this.updateMs - this.updateMsAvg) * 0.1;
  }

  private updateFrame(frame: SkyFrame): void {
    this.weather.update(frame.active, {
      pos: frame.camera.pos,
      lookAt: frame.camera.lookAt,
      cameraWorld: frame.cameraWorld,
    });
    this.hasLayers = false;
    for (const s of [...this.cylinder, ...this.dome, this.moon, this.sun, this.glow])
      s.mesh.visible = false;
    this.last = { layers: [], clouds: [], bodyDrawn: false, glowDrawn: false };
    this.flare.hide();
    if (!this.enabled) return;
    this.request(frame);
    const paths = this.textures.paths();

    // the sky view: the camera's rotation about the origin
    const view = skyViewLH(frame.camera.pos, frame.camera.lookAt);
    this.skyView = view;
    this.camera.matrixWorldInverse.fromArray(mirrorMatrix(view));
    this.camera.matrixWorld.copy(this.camera.matrixWorldInverse).invert();

    const [cu, cv] = this.cloudOffset();
    const layers: { kind: SkyKind; weight: number; drawn: boolean }[] = [];
    const clouds: { kind: SkyKind; weight: number; drawn: boolean }[] = [];
    frame.layers.layers.forEach((l, i) => {
      const w = l.weight / 255;
      const cyl = this.cylinder[i];
      const dom = this.dome[i];
      const skyTex = this.textures.texture(layerPath(paths, 'sky', l.kind));
      const cloudTex = this.textures.texture(layerPath(paths, 'cloud', l.kind));
      const drawSky = cyl !== undefined && l.weight > 0 && skyTex !== null;
      const drawCloud = dom !== undefined && l.weight > 0 && cloudTex !== null;
      if (cyl && drawSky) {
        cyl.material.uniforms.uMap.value = skyTex;
        cyl.material.uniforms.uWeight.value = w;
        cyl.mesh.visible = true;
        this.hasLayers = true;
      }
      if (dom && drawCloud) {
        dom.material.uniforms.uMap.value = cloudTex;
        dom.material.uniforms.uWeight.value = w;
        dom.material.uniforms.uUvOffset.value.set(cu, cv);
        dom.mesh.visible = true;
        this.hasLayers = true;
      }
      layers.push({ kind: l.kind, weight: l.weight, drawn: drawSky });
      clouds.push({ kind: l.kind, weight: l.weight, drawn: drawCloud });
    });

    // sun / moon: the angle comes from the clock (a continent light case reports 0 in the environment state)
    const angle = sunAngleOf(frame.hour, frame.min, frame.sec);
    this.body = bodyOfHour(frame.hour);
    let bodyDrawn = false;
    let glowDrawn = false;
    if (this.body === 'moon') {
      const tex = this.textures.texture(paths.moon);
      if (tex) {
        this.placeQuad(this.moon, rotatedQuad(angle, MOON_HALF, this.corners), tex);
        bodyDrawn = true;
        this.hasLayers = true;
      }
    } else if (this.body === 'sun') {
      const tex = this.textures.texture(paths.sun);
      if (tex) {
        this.placeQuad(this.sun, rotatedQuad(angle, SUN_HALF, this.corners), tex);
        bodyDrawn = true;
        this.hasLayers = true;
      }
      const glowTex = this.textures.texture(paths.glow);
      if (glowTex) {
        const centre = bodyPosition(angle);
        this.placeQuad(this.glow, billboardQuad(view, centre, GLOW_HALF, this.corners), glowTex);
        glowDrawn = true;
        this.hasLayers = true;
      }
    }
    this.last = { layers, clouds, bodyDrawn, glowDrawn };

    // the lens flare (pass 15)
    this.flare.update(
      { hour: frame.hour, min: frame.min, sec: frame.sec },
      angle,
      view,
      frame.camera.pos,
      viewCentre(frame.camera.pos, frame.camera.lookAt, 30),
    );
  }

  private placeQuad(slot: BodySlot, cornersLh: Float32Array, tex: Texture): void {
    mirrorQuadInto(cornersLh, this.mirrored);
    (slot.position.array as Float32Array).set(this.mirrored);
    slot.position.needsUpdate = true;
    slot.material.uniforms.uMap.value = tex;
    slot.mesh.visible = true;
  }

  tick(steps: number): void {
    if (this.disposed) return;
    const n = Math.max(0, Math.trunc(steps));
    this.cloudTicks += n;
    this.flare.tick(n);
    this.weather.tick(n);
  }

  /** The `StubSky`-compatible entry for the weather messages: the store values of `Environment` / `EnvironmentAll`. */
  weatherInput(season: number, effect: boolean): void {
    this.weather.input(season, effect);
  }

  /** What the specs and the probe read. */
  probe(): SkyProbe {
    const flare = this.flare.layout;
    return {
      enabled: this.enabled,
      layers: this.last.layers,
      clouds: this.last.clouds,
      cloudOffset: this.cloudOffset(),
      body: this.body,
      bodyDrawn: this.last.bodyDrawn,
      glowDrawn: this.last.glowDrawn,
      flare: {
        alpha: flare?.alpha ?? 0,
        drawn: this.flare.drawing,
        evaluations: this.flare.visibility.evaluations,
      },
      weather: {
        kind: this.weather.kind,
        locked: this.weather.controller.locked,
        season: this.weather.controller.season,
        drawing: this.weather.drawing,
        particles: this.weather.sim.count,
        ticks: this.weather.ticks,
        splashes: this.weather.sim.splashes,
        tickMs: this.weather.tickMs,
      },
      textures: this.textures.states(),
      cpuMs: { last: this.updateMs, avg: this.updateMsAvg },
    };
  }

  /** The sky view of the last frame (LH), `null` before the first. */
  get view(): Mat4 | null {
    return this.skyView;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const s of [...this.cylinder, ...this.dome]) s.material.dispose();
    for (const s of [this.moon, this.sun, this.glow]) s.material.dispose();
    for (const g of this.geometries) g.dispose();
    this.weather.dispose();
    this.flare.dispose();
    this.textures.dispose();
  }
}
