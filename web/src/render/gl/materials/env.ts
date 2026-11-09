// `EnvUniforms`: the lighting and fog uniforms shared by every model material (Plan/41 sections 7.2, 7.4, 8.1).
// The objects are shared BY REFERENCE between the materials, so updating the environment once per frame updates
// every material. WP-REND-07 computes the real `EnvLight` (light table, sun angle, fog, continents); until then a
// static default is used.

import { Vector2, Vector3 } from 'three';
import type { RGB } from '../../types';

/** The light and fog of one frame, after the boosts of section 8.2 (what the shaders consume). */
export interface EnvLight {
  /** Light diffuse used for shading (after the x1.1 / x1.2 / +0.1 boost). */
  readonly dif: RGB;
  /** Light ambient used for shading (after the x0.9 / x0.8 factor). */
  readonly amb: RGB;
  /** `m_lightFogSky.Diffuse` = the clear colour (before the boost). */
  readonly fogColor: RGB;
  readonly fogStart: number;
  readonly fogEnd: number;
  /** Direction the light travels, in three.js space. A zero vector gives no diffuse term (indoor worlds). */
  readonly dirThree: readonly [number, number, number];
  /** `hour >= 21 || hour <= 6`: light objects are drawn. */
  readonly night: boolean;
}

/** Noon (12:00:00) of the default outdoor light table (Plan/41 section 8.2 case 3): `T[10]` boosted. */
export const DEFAULT_ENV: EnvLight = {
  dif: [1.1, 1.1, 1.1],
  amb: [0.45, 0.45, 0.45],
  fogColor: [1, 1, 1],
  fogStart: 70,
  fogEnd: 400,
  // sunAngle at 12:00:00 = (43200 * 0.00015) - 90 = -83.52; dir = (0, -sin(96.48 deg), cos(96.48 deg)), z mirrored
  dirThree: [0, -0.9936113, 0.1128564],
  night: false,
};

interface Uniform<T> {
  value: T;
}

/** The uniform objects the model shaders declare. */
export interface EnvUniforms {
  /** `uDif`: light diffuse. */
  readonly uDif: Uniform<Vector3>;
  /** `uAmb`: light ambient. */
  readonly uAmb: Uniform<Vector3>;
  /** `uAmbRender`: `D3DRS_AMBIENT` = the ambient truncated to 8 bits (rigid models add it to the light ambient). */
  readonly uAmbRender: Uniform<Vector3>;
  /** `uSkinAmb`: `amb + 0.4` (vertex shader constant c94 of the skinning shader). */
  readonly uSkinAmb: Uniform<Vector3>;
  readonly uLightDir: Uniform<Vector3>;
  readonly uFogColor: Uniform<Vector3>;
  /** `(fogStart, fogEnd)`. */
  readonly uFogRange: Uniform<Vector2>;
  /** 1 at night. */
  readonly uNight: Uniform<number>;
}

const trunc8 = (v: number): number => Math.floor(Math.min(1, Math.max(0, v)) * 255) / 255;

/** Creates the shared uniforms, initialised from `env`. */
export function createEnvUniforms(env: EnvLight = DEFAULT_ENV): EnvUniforms {
  const u: EnvUniforms = {
    uDif: { value: new Vector3() },
    uAmb: { value: new Vector3() },
    uAmbRender: { value: new Vector3() },
    uSkinAmb: { value: new Vector3() },
    uLightDir: { value: new Vector3() },
    uFogColor: { value: new Vector3() },
    uFogRange: { value: new Vector2() },
    uNight: { value: 0 },
  };
  applyEnv(u, env);
  return u;
}

/** Writes `env` into the shared uniforms (once per drawn frame). */
export function applyEnv(u: EnvUniforms, env: EnvLight): void {
  u.uDif.value.set(env.dif[0], env.dif[1], env.dif[2]);
  u.uAmb.value.set(env.amb[0], env.amb[1], env.amb[2]);
  u.uAmbRender.value.set(trunc8(env.amb[0]), trunc8(env.amb[1]), trunc8(env.amb[2]));
  u.uSkinAmb.value.set(env.amb[0] + 0.4, env.amb[1] + 0.4, env.amb[2] + 0.4);
  u.uLightDir.value.set(env.dirThree[0], env.dirThree[1], env.dirThree[2]);
  u.uFogColor.value.set(env.fogColor[0], env.fogColor[1], env.fogColor[2]);
  u.uFogRange.value.set(env.fogStart, env.fogEnd);
  u.uNight.value = env.night ? 1 : 0;
}

/** The uniforms as a plain `uniforms` map entry set (shared by reference). */
export function envUniformMap(u: EnvUniforms): Record<string, Uniform<unknown>> {
  return { ...u };
}
