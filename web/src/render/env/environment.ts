// The environment state of a drawn frame (Plan/41 section 8, WP-REND-07; ref/19 section 19, World3D.cpp:1790-2024
// `CWorld::SetLight`, SkyBox.cpp:584-700, Neuz.cpp FrameMove): the light that shades terrain and models, the fog,
// the clear colour, the sun angle, the night flag and the sky cross-fade weights.
//
// `computeEnvironment` is a PURE function of the game clock, the world settings and an optional continent override;
// `EnvironmentTracker` holds the per-world inputs and the (REND-29) transition state and hands out the state of each
// frame. The colour numbers are the original floats (gamma-naive); the result converts to the shaders' `EnvLight`
// with `toEnvLight` (light direction mapped to three.js space).
//
// Light cases, first match wins (section 8.2):
//   1. inside an environment continent: amb = C_ambient, dif = C_diffuse -> transition blend -> fogColor = dif ->
//      dif *= 1.2, amb *= 0.8, dir = (0, -0.7071, -0.7071)                                       [WP-REND-29 detects it]
//   2. indoor world: dif = rgb(diffuse)/255, amb = rgb(ambient)/255 -> blend -> fogColor = dif -> dif += 0.1,
//      amb *= 0.9, dir = settings.light_dir (a zero vector gives no diffuse term)
//   3. outdoor: 24-entry table lerped by the minute -> blend -> fogColor = dif -> dif *= 1.1, amb *= 0.9,
//      dir = RotationX((sunAngle + 180) deg) * (0, 0, 1)

import type { ContinentDef, WorldSettingsDto } from '../../gen/bundles/types';
import { mirrorPos } from '../space';
import type { EnvLight } from '../gl/materials/env';
import type { GameClockSample, RGB, Vec3LH } from '../types';
import { skySchedule, type SkyLayers } from '../sky/schedule';
import { lightAt, lightTableOf, type LightEntry } from './lightTable';
import { isNight, lightDirLH, sunAngle } from './sunMoon';

export { isNight, lightDirLH, sunAngle } from './sunMoon';
export { DEFAULT_LIGHT_TABLE, lightAt, lightTableOf } from './lightTable';
export type { LightEntry } from './lightTable';

/** Weather kinds (Plan/41 section 15; the particle renderer is WP-REND-29). */
export type WeatherKind = 'none' | 'rain' | 'snow';

/** The part of the `.wld` settings the environment needs. */
export type EnvironmentSettings = Pick<
  WorldSettingsDto,
  'indoor' | 'ambient' | 'diffuse' | 'light_dir' | 'fog' | 'light24'
>;

/** The light of an environment continent (`C_ambient`, `C_diffuse`, `C_fogstartend`; floats 0..1, ref/19 section 11). */
export interface ContinentLight {
  readonly ambient: RGB;
  readonly diffuse: RGB;
  /** `C_fogstartend`, `null` keeps the world fog. */
  readonly fog: readonly [number, number] | null;
}

/** A light/fog setting that a transition lerps (`old - (old - target) * t`, section 8.6). */
export interface LightFog {
  readonly amb: RGB;
  readonly dif: RGB;
  readonly fogStart: number;
  readonly fogEnd: number;
}

/**
 * The transition of section 8.6 (continent enter/leave, 2 s): the base light of the frame is
 * `from - (from - target) * t`, where `target` is what the cases above compute and `from` the snapshot taken when the
 * transition began. REND-07 only provides the arithmetic; WP-REND-29 drives it.
 */
export interface EnvTransition {
  readonly from: LightFog;
  /** 0 = still `from`, 1 = done. */
  readonly t: number;
}

/** `EnvironmentState` of Plan/41 section 8.1. */
export interface EnvironmentState {
  /** The clock the state was computed for (WdGuildWar forces 19:00:00). */
  readonly hour: number;
  readonly min: number;
  readonly sec: number;
  /** `hour >= 21 || hour <= 6`. */
  readonly night: boolean;
  /** Light used for shading (after the boost). */
  readonly dif: RGB;
  readonly amb: RGB;
  /** `m_lightFogSky.Diffuse` (before the boost): fog colour = clear colour. */
  readonly fogColor: RGB;
  /** The direction the light travels, LH and three.js space. */
  readonly dirLH: Vec3LH;
  readonly dirThree: readonly [number, number, number];
  readonly fogStart: number;
  readonly fogEnd: number;
  /** `m_fSunAngle` as the sky render leaves it (outdoor); 0 indoors / in a continent. */
  readonly sunAngle: number;
  /** The sky cross-fade (outdoor worlds); empty indoors. */
  readonly sky: SkyLayers;
  readonly weather: WeatherKind;
  readonly weatherLocked: boolean;
  readonly indoor: boolean;
  /** Which of the three light cases produced the state (1 continent, 2 indoor, 3 outdoor). */
  readonly lightCase: 1 | 2 | 3;
  /** The continent whose light is active, `null` outside. */
  readonly continent: number | null;
}

const clamp01 = (v: number): number => Math.min(1, Math.max(0, v));

const scale3 = (c: RGB, k: number): RGB => [c[0] * k, c[1] * k, c[2] * k];
const add3 = (c: RGB, k: number): RGB => [c[0] + k, c[1] + k, c[2] + k];

/** `D3DCOLOR` 0xAARRGGBB as floats 0..1 (the alpha is ignored, ref/19 section 19.2 case 2). */
export function d3dColorRgb(c: number): RGB {
  const v = c >>> 0;
  return [((v >>> 16) & 0xff) / 255, ((v >>> 8) & 0xff) / 255, (v & 0xff) / 255];
}

/** The folder of the world whose clock the client forces to 19:00:00 (`WI_WORLD_GUILDWAR`, DPClient.cpp:2425-2436). */
export const GUILD_WAR_FOLDER = 'wdguildwar';

/** The clock the environment uses: WdGuildWar is fixed at 19:00:00, every other world follows the game clock. */
export function effectiveClock(folder: string, clock: GameClockSample): GameClockSample {
  if (folder.toLowerCase() === GUILD_WAR_FOLDER) return { hour: 19, min: 0, sec: 0 };
  return clock;
}

/** Everything `computeEnvironment` reads. */
export interface EnvironmentInput {
  /** World folder (lower-case). */
  readonly folder: string;
  readonly settings: EnvironmentSettings;
  readonly clock: GameClockSample;
  /** The continent light when the active mover is inside an environment continent (WP-REND-29), else `null`. */
  readonly continent?: (ContinentLight & { readonly id: number }) | null;
  /** The running continent transition, if any (WP-REND-29). */
  readonly transition?: EnvTransition | null;
  /** The decoded table of the world (`lightTableOf(settings.light24).table`), computed once per world. */
  readonly table?: readonly LightEntry[];
  /** Weather of the frame (WP-REND-29); defaults to none/unlocked. */
  readonly weather?: { readonly kind: WeatherKind; readonly locked: boolean };
}

/** The fixed continent light direction (0, -0.7071, -0.7071) in LH. */
export const CONTINENT_DIR_LH: Vec3LH = [0, -0.7071, -0.7071];

/** Lerp of the base light with the transition snapshot: `old - (old - target) * t`. */
function blend(target: LightFog, tr: EnvTransition | null | undefined): LightFog {
  if (!tr) return target;
  const t = clamp01(tr.t);
  const l = (from: number, to: number): number => from - (from - to) * t;
  const c = (from: RGB, to: RGB): RGB => [l(from[0], to[0]), l(from[1], to[1]), l(from[2], to[2])];
  return {
    amb: c(tr.from.amb, target.amb),
    dif: c(tr.from.dif, target.dif),
    fogStart: l(tr.from.fogStart, target.fogStart),
    fogEnd: l(tr.from.fogEnd, target.fogEnd),
  };
}

/** The environment of one frame (Plan/41 section 8). */
export function computeEnvironment(input: EnvironmentInput): EnvironmentState {
  const { settings } = input;
  const clock = effectiveClock(input.folder, input.clock);
  const worldFogStart = settings.fog[0];
  const worldFogEnd = settings.fog[1];
  const weather = input.weather ?? { kind: 'none' as const, locked: false };
  const night = isNight(clock.hour);

  let lightCase: 1 | 2 | 3;
  let base: LightFog;
  let dirLH: Vec3LH;
  let angle = 0;
  let continent: number | null = null;

  if (input.continent) {
    lightCase = 1;
    continent = input.continent.id;
    base = {
      amb: input.continent.ambient,
      dif: input.continent.diffuse,
      fogStart: input.continent.fog?.[0] ?? worldFogStart,
      fogEnd: input.continent.fog?.[1] ?? worldFogEnd,
    };
    dirLH = CONTINENT_DIR_LH;
  } else if (settings.indoor) {
    lightCase = 2;
    base = {
      amb: d3dColorRgb(settings.ambient),
      dif: d3dColorRgb(settings.diffuse),
      fogStart: worldFogStart,
      fogEnd: worldFogEnd,
    };
    dirLH = [settings.light_dir[0], settings.light_dir[1], settings.light_dir[2]];
  } else {
    lightCase = 3;
    const table = input.table ?? lightTableOf(settings.light24).table;
    const l = lightAt(table, clock.hour, clock.min);
    base = { amb: l.amb, dif: l.dif, fogStart: worldFogStart, fogEnd: worldFogEnd };
    angle = sunAngle(clock.hour, clock.min, clock.sec);
    dirLH = lightDirLH(angle);
  }

  const lit = blend(base, input.transition);
  // the fog and clear colour are the light diffuse BEFORE the boost
  const fogColor = lit.dif;
  let dif: RGB;
  let amb: RGB;
  switch (lightCase) {
    case 1:
      dif = scale3(lit.dif, 1.2);
      amb = scale3(lit.amb, 0.8);
      break;
    case 2:
      dif = add3(lit.dif, 0.1);
      amb = scale3(lit.amb, 0.9);
      break;
    case 3:
      dif = scale3(lit.dif, 1.1);
      amb = scale3(lit.amb, 0.9);
      break;
  }
  const m = mirrorPos(dirLH[0], dirLH[1], dirLH[2]);
  return {
    hour: clock.hour,
    min: clock.min,
    sec: clock.sec,
    night,
    dif,
    amb,
    fogColor,
    dirLH,
    dirThree: [m[0], m[1], m[2]],
    fogStart: lit.fogStart,
    fogEnd: lit.fogEnd,
    sunAngle: angle,
    sky: settings.indoor ? { layers: [] } : skySchedule(clock.hour, clock.min),
    weather: weather.kind,
    weatherLocked: weather.locked,
    indoor: settings.indoor,
    lightCase,
    continent,
  };
}

/** The shader-facing light of a state (Plan/41 section 8.1 `EnvUniforms`). */
export function toEnvLight(s: EnvironmentState): EnvLight {
  return {
    dif: s.dif,
    amb: s.amb,
    fogColor: s.fogColor,
    fogStart: s.fogStart,
    fogEnd: s.fogEnd,
    dirThree: s.dirThree,
    night: s.night,
  };
}

/**
 * Holds the environment inputs of the current world (settings, decoded table, continents) and answers one state per
 * drawn frame. The continent detection and the 2 s transitions of section 8.6 are WP-REND-29; `setContinentLight` and
 * `setTransition` are the hooks it uses, and `continents()` the data it reads. Before that WP the tracker ignores
 * continents (the outdoor and indoor cases only), as documented in the M2 visual-QA notes.
 */
export class EnvironmentTracker {
  private folderName = '';
  private settingsValue: EnvironmentSettings | null = null;
  private table: readonly LightEntry[] | undefined;
  private continentList: readonly ContinentDef[] = [];
  private continentLight: (ContinentLight & { id: number }) | null = null;
  private transitionValue: EnvTransition | null = null;
  private weatherValue: { kind: WeatherKind; locked: boolean } = { kind: 'none', locked: false };
  /** `true` when the world's `light24` override was present but unusable (the default table is used). */
  invalidLightTable = false;
  /** The state of the last `sample` (probe, tests). */
  last: EnvironmentState | null = null;

  /** Sets the world (`null` leaves it). Resets the transition state (no transition on entry, section 22.5). */
  setWorld(
    folder: string,
    settings: EnvironmentSettings | null,
    continents: readonly ContinentDef[] = [],
  ): void {
    this.folderName = folder.toLowerCase();
    this.settingsValue = settings;
    this.continentList = continents.filter((c) => c.use_envir);
    this.continentLight = null;
    this.transitionValue = null;
    this.weatherValue = { kind: 'none', locked: false };
    this.invalidLightTable = false;
    this.table = undefined;
    if (settings) {
      const t = lightTableOf(settings.light24);
      this.table = t.table;
      this.invalidLightTable = !t.valid;
    }
    this.last = null;
  }

  get folder(): string {
    return this.folderName;
  }

  /** The environment continents of the world (`C_useEnvir`), the data WP-REND-29 tests the player against. */
  continents(): readonly ContinentDef[] {
    return this.continentList;
  }

  setContinentLight(light: (ContinentLight & { id: number }) | null): void {
    this.continentLight = light;
  }

  setTransition(t: EnvTransition | null): void {
    this.transitionValue = t;
  }

  setWeather(kind: WeatherKind, locked: boolean): void {
    this.weatherValue = { kind, locked };
  }

  /** The state of this frame for `clock`; `null` before a world was set. */
  sample(clock: GameClockSample): EnvironmentState | null {
    if (!this.settingsValue) return null;
    const state = computeEnvironment({
      folder: this.folderName,
      settings: this.settingsValue,
      clock,
      continent: this.continentLight,
      transition: this.transitionValue,
      ...(this.table ? { table: this.table } : {}),
      weather: this.weatherValue,
    });
    this.last = state;
    return state;
  }
}
