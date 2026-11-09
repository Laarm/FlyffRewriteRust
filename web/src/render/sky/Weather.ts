// Rain and snow (Plan/41 section 15, WP-REND-29; SkyBox.cpp:997-1094 `DrawRain` / `DrawSnow`, pass 14). The state machine and the
// particle simulation are `weatherSim.ts`; this class owns the GPU side: one instanced draw of 300 / 400 quads.
//
//   rain   quad 0.01 wide x 2 tall (x in 0..0.01, y in 0..2 from the drop position), yaw-only billboard (the camera look direction
//          with `y = 0`), untextured grey 128, vertex alpha 0 (top) -> 100 (bottom), SRCALPHA / ONE, depth test on, write off
//   snow   quad 0.1 x 0.1 centred, full camera billboard, `sundisk.bmp` x 200, alpha 200, SRCALPHA / ONE
//
// Drawn only while the weather option is on (`RenderOptions.weatherEffect`, default OFF, KEEP) and the world allows weather
// (outdoor worlds and the Krrr dungeon). The simulation runs per 60 Hz cosmetic tick (FIX of the render-rate dependent original).

import {
  BufferAttribute,
  DynamicDrawUsage,
  InstancedBufferAttribute,
  InstancedBufferGeometry,
  Mesh,
  Scene,
} from 'three';
import { XI_GEN_RAINCIRCLE01 } from '../../gen/defs/obj';
import type { RenderFx } from '../fx';
import type { RenderPass } from '../passes';
import type { RenderOptions } from '../quality';
import type { Vec3LH } from '../types';
import { createFallMaterial, type FallMaterial } from './skyShaders';
import type { SkyTextures } from './skyTextures';
import { yawRight } from './skyView';
import {
  FallSim,
  WeatherController,
  particleCount,
  worldAllowsWeather,
  type FallGround,
  type WeatherKind,
  type WeatherWorld,
} from './weatherSim';

/** A cosmetic backlog (a hidden tab) must not simulate seconds of drops. */
export const MAX_STEPS_PER_FRAME = 8;

export interface WeatherDeps {
  readonly world: WeatherWorld;
  readonly options: () => RenderOptions;
  readonly textures: SkyTextures;
  readonly ground: FallGround;
  /** The effect API (rain splashes); may be absent in tests. */
  readonly fx?: () => RenderFx | undefined;
  /** `[0, 1)`; the specs seed it. */
  readonly rand?: () => number;
}

/** The corners of the rain quad (fan order) and the snow quad (top-left, top-right, bottom-right, bottom-left). */
const RAIN_QUAD = {
  positions: [0, 2, 0, 0.01, 2, 0, 0.01, 0, 0, 0, 0, 0],
  alphas: [0, 0, 100 / 255, 100 / 255],
};
const SNOW_QUAD = {
  positions: [-0.05, 0.05, 0, 0.05, 0.05, 0, 0.05, -0.05, 0, -0.05, -0.05, 0],
  alphas: [200 / 255, 200 / 255, 200 / 255, 200 / 255],
};
const QUAD_UV = [0, 0, 1, 0, 1, 1, 0, 1];
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];

function quadGeometry(
  q: { positions: number[]; alphas: number[] },
  capacity: number,
): {
  geometry: InstancedBufferGeometry;
  offsets: InstancedBufferAttribute;
} {
  const g = new InstancedBufferGeometry();
  g.setAttribute('position', new BufferAttribute(new Float32Array(q.positions), 3));
  g.setAttribute('uv', new BufferAttribute(new Float32Array(QUAD_UV), 2));
  g.setAttribute('aAlpha', new BufferAttribute(new Float32Array(q.alphas), 1));
  g.setIndex(new BufferAttribute(new Uint16Array(QUAD_INDEX), 1));
  const offsets = new InstancedBufferAttribute(new Float32Array(capacity * 3), 3);
  offsets.setUsage(DynamicDrawUsage);
  g.setAttribute('aOffset', offsets);
  g.instanceCount = 0;
  return { geometry: g, offsets };
}

/** The per-frame camera facts the billboards need. */
export interface WeatherView {
  /** Camera position and look-at, LH. */
  readonly pos: Vec3LH;
  readonly lookAt: Vec3LH;
  /** The camera's world matrix elements in three.js space (column 0 = right, column 1 = up). */
  readonly cameraWorld: ArrayLike<number>;
}

export class Weather {
  readonly controller: WeatherController;
  readonly sim: FallSim;
  readonly scene = new Scene();
  private readonly rain: { mesh: Mesh; material: FallMaterial; offsets: InstancedBufferAttribute };
  private readonly snow: { mesh: Mesh; material: FallMaterial; offsets: InstancedBufferAttribute };
  private player: Vec3LH | null = null;
  private right: [number, number] = [1, 0];
  private dirty = false;
  private drawn: 'rain' | 'snow' | null = null;
  /** Particle steps simulated so far (probe, tests). */
  ticks = 0;
  /** Milliseconds per simulated step, moving average (the weather's share of the cosmetic budget). */
  tickMs = 0;

  constructor(private readonly deps: WeatherDeps) {
    this.controller = new WeatherController(deps.world);
    this.sim = new FallSim(deps.rand ?? Math.random);
    const mk = (snow: boolean): Weather['rain'] => {
      const { geometry, offsets } = quadGeometry(snow ? SNOW_QUAD : RAIN_QUAD, 2000);
      const material = createFallMaterial(snow ? 'weather-snow' : 'weather-rain', snow);
      const mesh = new Mesh(geometry, material);
      mesh.frustumCulled = false;
      mesh.matrixAutoUpdate = false;
      mesh.visible = false;
      this.scene.add(mesh);
      return { mesh, material, offsets };
    };
    this.rain = mk(false);
    this.snow = mk(true);
    this.scene.matrixAutoUpdate = false;
    this.scene.matrixWorldAutoUpdate = false;
  }

  /** The world allows weather and the option is on (the controller state still follows the messages when it is off). */
  get enabled(): boolean {
    return this.deps.options().weatherEffect && worldAllowsWeather(this.deps.world);
  }

  get kind(): WeatherKind {
    return this.controller.kind;
  }

  /** `true` when a draw call is issued this frame. */
  get drawing(): boolean {
    return this.drawn !== null;
  }

  /** The store values of `Environment` / `EnvironmentAll` (`worldStore.env`). */
  input(season: number, effect: boolean): void {
    this.controller.input(season, effect);
  }

  /** One drawn frame: where the player is, the billboard axes, the instance buffer. */
  update(player: Vec3LH, view: WeatherView): void {
    this.player = player;
    this.drawn = null;
    this.rain.mesh.visible = false;
    this.snow.mesh.visible = false;
    if (!this.enabled || this.sim.count === 0) return;
    if (this.controller.kind === 'rain' && this.sim.kind === 'rain') {
      this.right = yawRight(view.pos, view.lookAt, this.right);
      // LH -> three.js: z is negated
      this.rain.material.uniforms.uRight.value.set(this.right[0], 0, -this.right[1]);
      this.rain.material.uniforms.uUp.value.set(0, 1, 0);
      this.upload(this.rain.offsets, this.rain.mesh);
      this.rain.mesh.visible = true;
      this.drawn = 'rain';
    } else if (this.controller.kind === 'snow' && this.sim.kind === 'snow') {
      const tex = this.deps.textures.texture(this.deps.textures.paths().snow);
      if (!tex) return;
      const e = view.cameraWorld;
      const u = this.snow.material.uniforms;
      u.uRight.value.set(e[0] ?? 1, e[1] ?? 0, e[2] ?? 0);
      u.uUp.value.set(e[4] ?? 0, e[5] ?? 1, e[6] ?? 0);
      u.uMap.value = tex;
      this.upload(this.snow.offsets, this.snow.mesh);
      this.snow.mesh.visible = true;
      this.drawn = 'snow';
    }
  }

  private upload(offsets: InstancedBufferAttribute, mesh: Mesh): void {
    const n = this.sim.count;
    (mesh.geometry as InstancedBufferGeometry).instanceCount = n;
    if (!this.dirty && offsets.version > 0) return;
    const a = offsets.array as Float32Array;
    for (let i = 0; i < n; i++) {
      a[i * 3] = this.sim.px[i] ?? 0;
      a[i * 3 + 1] = this.sim.py[i] ?? 0;
      a[i * 3 + 2] = -(this.sim.pz[i] ?? 0);
    }
    offsets.clearUpdateRanges();
    offsets.addUpdateRange(0, n * 3);
    offsets.needsUpdate = true;
    this.dirty = false;
  }

  /** Cosmetic 60 Hz steps. */
  tick(steps: number): void {
    // the option gates the simulation and the drawing (`Process` and `RenderFall`); the kind itself follows the messages in `input`
    if (!this.enabled) return;
    this.controller.process();
    const player = this.player;
    if (player === null) return;
    const kind = this.controller.kind;
    if (this.controller.takeReinit() || this.sim.kind !== kind) {
      this.sim.init(kind, particleCount(this.deps.options().textureQuality), player);
      this.dirty = true;
    }
    if (kind === 'none') return;
    const n = Math.min(MAX_STEPS_PER_FRAME, Math.max(0, Math.trunc(steps)));
    const fx = this.deps.fx?.();
    const t0 = performance.now();
    for (let i = 0; i < n; i++) {
      this.sim.tick(player, this.deps.ground, (s) => {
        fx?.sfx(XI_GEN_RAINCIRCLE01, {
          pos: s.pos,
          scale: s.scale,
          ...(s.rotateDeg ? { rotateDeg: s.rotateDeg } : {}),
        });
      });
      this.ticks++;
    }
    if (n > 0) {
      this.dirty = true;
      this.tickMs += ((performance.now() - t0) / n - this.tickMs) * 0.1;
    }
  }

  /** The pass body (pass 14). */
  readonly pass: RenderPass = {
    name: 'weather',
    enabled: true,
    render: (ctx) => {
      if (this.drawn === null) return;
      ctx.device.renderLayer(this.scene, ctx.camera.camera, null, false);
    },
  };

  dispose(): void {
    for (const part of [this.rain, this.snow]) {
      part.mesh.geometry.dispose();
      part.material.dispose();
      this.scene.remove(part.mesh);
    }
  }
}
