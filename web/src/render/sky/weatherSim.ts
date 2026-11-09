// The weather state machine and the particle simulation (Plan/41 section 15; ref/19 section 21; SkyBox.cpp:60-277 `SetWeather`,
// `InitFall`, `Process`; DPClient.cpp:5711-5776 `OnEnvironmentEffect`). Pure logic: no GL, a random source and a ground query are
// injected, so the spawn ranges, velocities and respawn rules are unit-tested with a seeded generator.
//
//   WeatherController   what `OnEnvironmentSetting` / `OnEnvironmentEffect` / `SetWeather` / the Kebaras rule of `Process` do:
//                       season SUMMER -> RAIN, SPRING / FALL / WINTER -> SNOW, NONE -> nothing; an indoor world ignores the
//                       messages except the Krrr dungeon; `lock` (continent weather, REND-29) freezes the kind.
//   FallSim             the 300 / 400 particles: rain spawns within +-12.5 of the player (x, z) and 10..20 above him, falls with
//                       `(-(7..16) / 200 - 0.05) * 1.5` per tick and splashes (30 %) when it passes below the ground;
//                       snow spawns within +-35 and 10..30 above, drifts with `(x/800, -(0..9)/200 - 0.05, z/800)`.
//
// With the shipped data `EnvironmentEffect.txt` yields SEASON_NONE for every month, so a server toggle shows nothing except in
// WdKebaras, which snows (KEEP, documented for testers).

import {
  SEASON_FALL,
  SEASON_NONE,
  SEASON_SPRING,
  SEASON_SUMMER,
  SEASON_WINTER,
} from '../../gen/defs/define';
import { WI_DUNGEON_KRRR, WI_WORLD_KEBARAS } from '../../gen/defs/world';
import type { WeatherKind } from '../env/environment';
import type { Vec3LH } from '../types';

export type { WeatherKind } from '../env/environment';

/** Particle count by texture quality: 400 at 0, else 300 (SkyBox.cpp:19-38). */
export function particleCount(textureQuality: 0 | 1 | 2): number {
  return textureQuality === 0 ? 400 : 300;
}

/** `OnEnvironmentEffect`'s season table. */
export function weatherOfSeason(season: number): WeatherKind {
  switch (season) {
    case SEASON_SUMMER:
      return 'rain';
    case SEASON_SPRING:
    case SEASON_FALL:
    case SEASON_WINTER:
      return 'snow';
    default:
      return 'none';
  }
}

/** The world facts the controller needs. */
export interface WeatherWorld {
  readonly worldId: number;
  readonly indoor: boolean;
}

/** `true` when the world processes and draws weather: outdoor worlds and the Krrr dungeon. */
export function worldAllowsWeather(world: WeatherWorld): boolean {
  return !world.indoor || world.worldId === WI_DUNGEON_KRRR;
}

export class WeatherController {
  /** `m_nWeather`. */
  kind: WeatherKind = 'none';
  /** `m_bLockWeather` (continent weather, WP-REND-29). */
  locked = false;
  /** `CEnvironment::GetSeason()`. */
  season = SEASON_NONE;
  /** Particles must be re-initialised (`InitFall`): set by a kind change, cleared by `takeReinit`. */
  private reinit = false;
  private seenSeason = SEASON_NONE;
  private seenEffect = false;

  constructor(private readonly world: WeatherWorld) {}

  /** `CSkyBox::SetWeather(kind, on)`. */
  setWeather(kind: WeatherKind, on: boolean): void {
    if (this.locked) return;
    if (!on) {
      if (kind === this.kind) this.kind = 'none';
    } else if (this.kind !== kind) {
      this.kind = kind;
      this.reinit = true;
    }
  }

  /**
   * The server messages as the stores hold them: `EnvironmentAll.season` is stored, `Environment.effect` runs `OnEnvironmentEffect`.
   * Changes are detected here (the message handlers only write the stores); the season is applied before the effect.
   */
  input(season: number, effect: boolean): void {
    if (season !== this.seenSeason) {
      this.seenSeason = season;
      this.season = season;
    }
    if (effect !== this.seenEffect) {
      this.seenEffect = effect;
      this.onEffect(effect);
    }
  }

  /** `OnEnvironmentEffect`: an indoor world (other than the Krrr dungeon) ignores it. */
  private onEffect(effect: boolean): void {
    if (!worldAllowsWeather(this.world)) return;
    this.setWeather(weatherOfSeason(this.season), effect);
  }

  /**
   * The Kebaras rule of `CSkyBox::Process` (runs only while the weather option is on): the world always snows, which also sets the
   * client season to WINTER for good (KEEP: later weather toggles in other worlds snow too).
   */
  process(): void {
    if (this.world.worldId === WI_WORLD_KEBARAS && this.season !== SEASON_WINTER) {
      this.season = SEASON_WINTER;
      this.setWeather('snow', true);
    }
  }

  /** Returns `true` once after a kind change (the caller re-initialises the particles). */
  takeReinit(): boolean {
    const r = this.reinit;
    this.reinit = false;
    return r;
  }
}

// ---- particles ------------------------------------------------------------------------------------------

/** Ground queries of the simulation (`render/terrain/heightField.ts` in the renderer; a fake in the tests). */
export interface FallGround {
  /** `GetLandHeight_Fast`. */
  heightFast(x: number, z: number): number;
  /** `GetLandHeight` (bilinear). */
  heightAt(x: number, z: number): number;
  /** The water patch under `(x, z)`: `type === 2` is water; `null` when no land is resident. */
  waterAt(x: number, z: number): { readonly height: number; readonly type: number } | null;
  /** `GetLandTri`: the three vertices; `null` when no land is resident. */
  triangleAt(
    x: number,
    z: number,
  ): { readonly a: Vec3LH; readonly b: Vec3LH; readonly c: Vec3LH } | null;
}

/** A rain splash (`XI_GEN_RAINCIRCLE01`). */
export interface Splash {
  readonly pos: Vec3LH;
  readonly scale: number;
  /** `m_vRotate` in degrees (ground splashes tilt to the terrain normal), `null` on water. */
  readonly rotateDeg: Vec3LH | null;
}

const RAIN_SPREAD = 25;
const SNOW_SPREAD = 70;
/** Rain moves `velocity * 1.5` per tick, snow `velocity * 1`. */
const RAIN_SPEED = 1.5;

/** `D3DXVec3Cross`. */
const cross = (a: Vec3LH, b: Vec3LH): [number, number, number] => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

/** `QuaternionRotationToYPW` (xUtil3D.cpp:7), radians `(pitch, yaw, roll)`. */
export function quaternionToYpw(
  q: readonly [number, number, number, number],
): [number, number, number] {
  const [x, y, z, w] = q;
  const sqw = w * w;
  const sqx = x * x;
  const sqy = y * y;
  const sqz = z * z;
  // asinf of a value that rounding pushes slightly past 1 would give NaN
  const s = Math.max(-1, Math.min(1, 2 * (w * x - y * z)));
  return [
    Math.asin(s),
    Math.atan2(2 * (x * z + w * y), -sqx - sqy + sqz + sqw),
    Math.atan2(2 * (x * y + w * z), -sqx + sqy - sqz + sqw),
  ];
}

/**
 * The tilt of a ground splash (SkyBox.cpp:158-198): the face normal `cross(c - a, b - a)` of the triangle under the drop, the
 * rotation from `(0, -1, 0)` to it (`acos(dot)` about `cross`), as yaw / pitch / roll in degrees `(x = pitch, y = yaw, z = roll)`.
 */
export function splashTilt(tri: {
  readonly a: Vec3LH;
  readonly b: Vec3LH;
  readonly c: Vec3LH;
}): Vec3LH {
  const v1: Vec3LH = [tri.c[0] - tri.a[0], tri.c[1] - tri.a[1], tri.c[2] - tri.a[2]];
  const v2: Vec3LH = [tri.b[0] - tri.a[0], tri.b[1] - tri.a[1], tri.b[2] - tri.a[2]];
  let n = cross(v1, v2);
  const len = Math.hypot(n[0], n[1], n[2]);
  n = len > 0 ? [n[0] / len, n[1] / len, n[2] / len] : [0, -1, 0];
  const up: Vec3LH = [0, -1, 0];
  const axis = cross(up, n);
  const dot = up[0] * n[0] + up[1] * n[1] + up[2] * n[2];
  const theta = Math.acos(Math.max(-1, Math.min(1, dot)));
  // `D3DXQuaternionRotationAxis`: a zero axis stays zero
  const al = Math.hypot(axis[0], axis[1], axis[2]);
  const s = Math.sin(theta / 2);
  const q: [number, number, number, number] =
    al > 0
      ? [(axis[0] / al) * s, (axis[1] / al) * s, (axis[2] / al) * s, Math.cos(theta / 2)]
      : [0, 0, 0, Math.cos(theta / 2)];
  const [p, y, r] = quaternionToYpw(q);
  const deg = 180 / Math.PI;
  return [p * deg, y * deg, r * deg];
}

/** The particle arrays of one weather effect. */
export class FallSim {
  /** Positions (LH world) and per-tick velocities. */
  readonly px = new Float32Array(2000);
  readonly py = new Float32Array(2000);
  readonly pz = new Float32Array(2000);
  readonly vx = new Float32Array(2000);
  readonly vy = new Float32Array(2000);
  readonly vz = new Float32Array(2000);
  count = 0;
  kind: WeatherKind = 'none';
  /** Splashes requested since creation (probe, tests). */
  splashes = 0;

  /** `rand` returns `[0, 1)`; the specs seed it. */
  constructor(private readonly rand: () => number) {}

  /** `xRandom(n)`: an integer in `[0, n)`. */
  private xRandom(n: number): number {
    return Math.floor(this.rand() * n);
  }

  private spawn(i: number, player: Vec3LH, spread: number, rise: number): void {
    this.px[i] = player[0] + (this.rand() - 0.5) * spread;
    this.pz[i] = player[2] + (this.rand() - 0.5) * spread;
    this.py[i] = player[1] + 10 + this.rand() * rise;
  }

  /** `InitFall`: positions around the player and the velocities of the kind. */
  init(kind: WeatherKind, count: number, player: Vec3LH): void {
    this.kind = kind;
    this.count = kind === 'none' ? 0 : Math.max(0, Math.min(2000, Math.trunc(count)));
    for (let i = 0; i < this.count; i++) {
      this.spawn(i, player, RAIN_SPREAD, 10);
      if (kind === 'rain') {
        const y = -this.xRandom(10) - 7;
        this.vx[i] = 0;
        this.vy[i] = y / 200 - 0.05;
        this.vz[i] = 0;
      } else {
        const x = this.xRandom(50) - 25;
        const y = -this.xRandom(10);
        const z = this.xRandom(50) - 25;
        this.vx[i] = x / 800;
        this.vy[i] = y / 200 - 0.05;
        this.vz[i] = z / 800;
      }
    }
  }

  /** One 60 Hz step of `CSkyBox::Process` for the current kind. */
  tick(player: Vec3LH, ground: FallGround, onSplash?: (s: Splash) => void): void {
    if (this.kind === 'rain') this.tickRain(player, ground, onSplash);
    else if (this.kind === 'snow') this.tickSnow(player, ground);
  }

  private tickRain(player: Vec3LH, ground: FallGround, onSplash?: (s: Splash) => void): void {
    for (let i = 0; i < this.count; i++) {
      const x = this.px[i] ?? 0;
      const z = this.pz[i] ?? 0;
      const floor = ground.heightFast(x, z) - 1;
      if (floor > (this.py[i] ?? 0)) {
        if (this.xRandom(10) >= 7) this.splash(x, z, ground, onSplash);
        this.spawn(i, player, RAIN_SPREAD, 10);
      } else {
        this.px[i] = x + (this.vx[i] ?? 0) * RAIN_SPEED;
        this.py[i] = (this.py[i] ?? 0) + (this.vy[i] ?? 0) * RAIN_SPEED;
        this.pz[i] = z + (this.vz[i] ?? 0) * RAIN_SPEED;
      }
    }
  }

  private splash(x: number, z: number, ground: FallGround, onSplash?: (s: Splash) => void): void {
    const water = ground.waterAt(x, z);
    // no resident land: nothing to splash on (the original read stale triangle memory)
    if (water === null) return;
    this.splashes++;
    if (!onSplash) return;
    if (water.type === 2) {
      const scale = (this.xRandom(2) + 1) * 0.1;
      onSplash({ pos: [x, water.height, z], scale, rotateDeg: null });
      return;
    }
    const tri = ground.triangleAt(x, z);
    const scale = (this.xRandom(2) + 0.05) * 0.1;
    onSplash({
      pos: [x, ground.heightAt(x, z) + 0.1, z],
      scale,
      rotateDeg: tri ? splashTilt(tri) : null,
    });
  }

  private tickSnow(player: Vec3LH, ground: FallGround): void {
    for (let i = 0; i < this.count; i++) {
      const x = this.px[i] ?? 0;
      const z = this.pz[i] ?? 0;
      const floor = ground.heightFast(x, z) - 1;
      if (floor > (this.py[i] ?? 0)) {
        this.spawn(i, player, SNOW_SPREAD, 20);
      } else {
        this.px[i] = x + (this.vx[i] ?? 0);
        this.py[i] = (this.py[i] ?? 0) + (this.vy[i] ?? 0);
        this.pz[i] = z + (this.vz[i] ?? 0);
      }
    }
  }
}
