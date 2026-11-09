// Sun, moon and the sun glow (Plan/41 section 14.4; SkyBox.cpp:560-745). All quads are built at `z = 30` and turned by
// `D3DXMatrixRotationX(m_fSunAngle)` with the angle in RADIANS (the original uses the same number as degrees for the light
// direction, `env/sunMoon.ts`): the sun sweeps almost a full circle during the day, most of it outside the view. The glow is a
// camera-facing billboard of the sky view at the sun position.
//
//   hours 20..24 and 1..6   moon: quad +-2, `Moon.dds`, colour = texture, alpha = texture alpha, blend SRCALPHA / ONE
//   hours 7..17             sun: quad +-1, `sundisk.bmp`, colour = texture, blend ONE / ONE;
//                           glow: quad +-5 billboard, `Flare_3.tga`, colour = texture, alpha = texture alpha, SRCALPHA / INVSRCALPHA
//   hours 18, 19            neither

import { inverse, rotationX, transformCoord, type Mat4 } from '../d3dx';

export type Body = 'sun' | 'moon' | 'none';

/** The body drawn at game hour `hour` (1..24). */
export function bodyOfHour(hour: number): Body {
  if (hour >= 20 || (hour >= 1 && hour <= 6)) return 'moon';
  if (hour >= 7 && hour <= 17) return 'sun';
  return 'none';
}

/** `z` of the quads before the rotation. */
export const BODY_DISTANCE = 30;
/** Half sizes. */
export const SUN_HALF = 1;
export const MOON_HALF = 2;
export const GLOW_HALF = 5;

/** The centre of a body: `RotX(angle) * (0, 0, 30)`. */
export function bodyPosition(angle: number): [number, number, number] {
  return transformCoord([0, 0, BODY_DISTANCE], rotationX(angle));
}

/**
 * The four corners (top-left, top-right, bottom-right, bottom-left, as `D3DSUNVERTEX` order) of the quad of half size `half`
 * after `RotationX(angle)`, LH, 12 floats written to `out`.
 */
export function rotatedQuad(
  angle: number,
  half: number,
  out: Float32Array = new Float32Array(12),
): Float32Array {
  const m = rotationX(angle);
  const corners: [number, number][] = [
    [-half, half],
    [half, half],
    [half, -half],
    [-half, -half],
  ];
  corners.forEach(([x, y], i) => {
    const p = transformCoord([x, y, BODY_DISTANCE], m);
    out[i * 3] = p[0];
    out[i * 3 + 1] = p[1];
    out[i * 3 + 2] = p[2];
  });
  return out;
}

/**
 * The corners of a billboard of half size `half` centred at `centre`: the sky view's rotation inverted with the translation
 * replaced by `centre` (`matWorld = inverse(view); matWorld._41.._43 = centre`), LH, 12 floats written to `out`.
 */
export function billboardQuad(
  skyView: Mat4,
  centre: readonly [number, number, number],
  half: number,
  out: Float32Array = new Float32Array(12),
): Float32Array {
  const m = inverse(skyView);
  m[12] = centre[0];
  m[13] = centre[1];
  m[14] = centre[2];
  const corners: [number, number][] = [
    [-half, half],
    [half, half],
    [half, -half],
    [-half, -half],
  ];
  corners.forEach(([x, y], i) => {
    const p = transformCoord([x, y, 0], m);
    out[i * 3] = p[0];
    out[i * 3 + 1] = p[1];
    out[i * 3 + 2] = p[2];
  });
  return out;
}

/** Writes `src` (LH, 12 floats) mirrored to three.js space into `dst` (`z` negated). */
export function mirrorQuadInto(src: Float32Array, dst: Float32Array): void {
  for (let i = 0; i < 4; i++) {
    dst[i * 3] = src[i * 3] ?? 0;
    dst[i * 3 + 1] = src[i * 3 + 1] ?? 0;
    dst[i * 3 + 2] = -(src[i * 3 + 2] ?? 0);
  }
}
