// Hair colour and shading of players (Plan/41 section 12.5, WP-REND-05; `CMover::SetHairColor`,
// MoverEquip.cpp:43-67, 156-221; ModelObject.cpp:1147-1184).
//
// A hair colour of exactly white (1, 1, 1) (within 0.001; the value a fresh character has) is replaced by the
// default table of its sex indexed by the hair mesh. The float setter used by the character creation / beauty shop
// previews clamps every other component to >= 0.3; the network setter (`SetHairColor(DWORD)`) does not, **KEEP**.
//
// Shading: the hair element is skinned with diffuse 0 and ambient `min(c * fblack, 1)`, `fblack = max(0.3,
// ((dif.r + dif.g + dif.b) * 1.5 / 3) * 0.6)` with `dif` the boosted world light diffuse; with world lighting off
// (previews) the ambient is the colour itself. The GPU evaluation is the `HAIR` branch of `modelShader.ts`;
// `hairAmbient` is the CPU reference used by the tests and the read-back checks.

import type { RGB } from '../types';

/** `nMaleHairColor[10][3]` (bytes). */
export const MALE_HAIR_COLORS: readonly (readonly [number, number, number])[] = [
  [144, 99, 101],
  [100, 107, 125],
  [158, 119, 109],
  [199, 156, 100],
  [100, 123, 117],
  [100, 100, 100],
  [100, 100, 100],
  [100, 100, 100],
  [100, 100, 100],
  [100, 100, 100],
];

/** `nFeMaleHairColor[10][3]` (bytes). */
export const FEMALE_HAIR_COLORS: readonly (readonly [number, number, number])[] = [
  [250, 250, 90],
  [250, 250, 90],
  [147, 118, 103],
  [150, 100, 85],
  [213, 130, 100],
  [100, 100, 100],
  [100, 100, 100],
  [100, 100, 100],
  [100, 100, 100],
  [100, 100, 100],
];

/** `g_fHairLight`. */
export const HAIR_LIGHT = 1.5;

/** The default colour of a hair mesh (bytes / 255); an out-of-range mesh uses the last row. */
export function defaultHairColor(sex: 0 | 1, hairMesh: number): RGB {
  const table = sex === 0 ? MALE_HAIR_COLORS : FEMALE_HAIR_COLORS;
  const i = Math.min(table.length - 1, Math.max(0, Math.trunc(hairMesh)));
  const [r, g, b] = table[i] ?? [100, 100, 100];
  return [r / 255, g / 255, b / 255];
}

const isWhite = (r: number, g: number, b: number): boolean =>
  (r === 1 && g === 1 && b === 1) ||
  (Math.abs(r - 1) < 0.001 && Math.abs(g - 1) < 0.001 && Math.abs(b - 1) < 0.001);

/**
 * The colour of `SetHairColor(DWORD)` (the network path): `0x00RRGGBB` components / 255, white replaced by the
 * default table of the hair mesh, nothing clamped.
 */
export function hairColorFromDword(dwHairColor: number, sex: 0 | 1, hairMesh: number): RGB {
  const r = ((dwHairColor >>> 16) & 0xff) / 255;
  const g = ((dwHairColor >>> 8) & 0xff) / 255;
  const b = (dwHairColor & 0xff) / 255;
  return isWhite(r, g, b) ? defaultHairColor(sex, hairMesh) : [r, g, b];
}

/**
 * The colour of `SetHairColor(FLOAT r, g, b)` (character creation / beauty shop previews): white is the default
 * table, every other component is clamped to at least 0.3.
 */
export function hairColorFromFloats(
  r: number,
  g: number,
  b: number,
  sex: 0 | 1,
  hairMesh: number,
): RGB {
  if (isWhite(r, g, b)) return defaultHairColor(sex, hairMesh);
  return [Math.max(0.3, r), Math.max(0.3, g), Math.max(0.3, b)];
}

/** `fblack` of the hair shading from the boosted world diffuse `dif` (`g_fHairLight = 1.5`, floor 0.3). */
export function hairFblack(dif: RGB): number {
  const f = (dif[0] * HAIR_LIGHT + dif[1] * HAIR_LIGHT + dif[2] * HAIR_LIGHT) / 3;
  return Math.max(0.3, f * 0.6);
}

/**
 * The vertex colour of the hair element before the texture: `min(c * fblack, 1)` with world lighting, the colour
 * itself without (previews).
 */
export function hairAmbient(color: RGB, dif: RGB, lit: boolean): RGB {
  if (!lit) return [color[0], color[1], color[2]];
  const k = hairFblack(dif);
  return [Math.min(color[0] * k, 1), Math.min(color[1] * k, 1), Math.min(color[2] * k, 1)];
}
