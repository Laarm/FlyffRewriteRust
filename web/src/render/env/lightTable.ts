// The 24-entry outdoor light table (Plan/41 section 8.2 case 3, ref/19 section 19.2, `gs_kLight` in
// WorldEnvironment.cpp:16-43) and its minute lerp. A world may override the table (`use24light`, the `.24l` file:
// `WorldSettingsDto.light24`; no such file exists in the shipped data, the path is exercised by tests only).

import type { RGB } from '../types';

/** `[dr, dg, db, ar, ag, ab]`: light diffuse and ambient of one hour (0..1 floats, the original `r1g1b1 r2g2b2`). */
export type LightEntry = readonly [number, number, number, number, number, number];

const E = (a: number, b: number, c: number, d: number, e: number, f: number): LightEntry => [
  a,
  b,
  c,
  d,
  e,
  f,
];

/** `gs_kLight`: index = game hour - 1. */
export const DEFAULT_LIGHT_TABLE: readonly LightEntry[] = [
  // 0-5
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
  // 6
  E(0.5, 0.5, 0.6, 0.4, 0.4, 0.4),
  // 7-10
  E(0.7, 0.7, 0.7, 0.5, 0.5, 0.5),
  E(0.8, 0.8, 0.8, 0.5, 0.5, 0.5),
  E(0.9, 0.9, 0.9, 0.5, 0.5, 0.5),
  E(1, 1, 1, 0.5, 0.5, 0.5),
  // 11-14
  E(1, 1, 1, 0.6, 0.6, 0.6),
  E(1, 1, 1, 0.6, 0.6, 0.6),
  E(1, 1, 1, 0.6, 0.6, 0.6),
  E(1, 1, 1, 0.6, 0.6, 0.6),
  // 15-23
  E(1, 1, 1, 0.5, 0.5, 0.5),
  E(0.9, 0.9, 0.9, 0.5, 0.5, 0.5),
  E(0.9, 0.6, 0.2, 0.5, 0.5, 0.4),
  E(0.6, 0.6, 0.4, 0.4, 0.4, 0.4),
  E(0.5, 0.5, 0.4, 0.4, 0.4, 0.4),
  E(0.45, 0.45, 0.4, 0.35, 0.35, 0.35),
  E(0.43, 0.43, 0.5, 0.33, 0.33, 0.3),
  E(0.41, 0.41, 0.5, 0.31, 0.31, 0.3),
  E(0.4, 0.4, 0.5, 0.3, 0.3, 0.4),
];

/**
 * The table of a world: `settings.light24` when it is a valid 24 x 6 table of finite numbers, else the default one.
 * `valid` is `false` when an override was given but unusable (the caller may log it once).
 */
export function lightTableOf(light24: readonly (readonly number[])[] | null | undefined): {
  readonly table: readonly LightEntry[];
  readonly overridden: boolean;
  readonly valid: boolean;
} {
  if (light24 === null || light24 === undefined) {
    return { table: DEFAULT_LIGHT_TABLE, overridden: false, valid: true };
  }
  const ok =
    light24.length === 24 &&
    light24.every((r) => r.length === 6 && r.every((v) => Number.isFinite(v)));
  if (!ok) return { table: DEFAULT_LIGHT_TABLE, overridden: true, valid: false };
  return {
    table: light24.map((r) => E(r[0] ?? 0, r[1] ?? 0, r[2] ?? 0, r[3] ?? 0, r[4] ?? 0, r[5] ?? 0)),
    overridden: true,
    valid: true,
  };
}

/** The lerp of case 3: `k = clamp(hour - 1, 0, 23)`, from `table[k - 1]` (wrapping) to `table[k]` by `min / 60`. */
export function lightAt(
  table: readonly LightEntry[],
  hour: number,
  min: number,
): { readonly dif: RGB; readonly amb: RGB } {
  const k = Math.min(23, Math.max(0, Math.trunc(hour) - 1));
  const prev = k === 0 ? 23 : k - 1;
  const a = table[prev] ?? DEFAULT_LIGHT_TABLE[0] ?? E(0, 0, 0, 0, 0, 0);
  const b = table[k] ?? DEFAULT_LIGHT_TABLE[0] ?? E(0, 0, 0, 0, 0, 0);
  // the original lerps with the integer minute (steps of 1/60)
  const f = Math.min(59, Math.max(0, Math.trunc(min))) / 60;
  const at = (i: 0 | 1 | 2 | 3 | 4 | 5): number => a[i] + (b[i] - a[i]) * f;
  return { dif: [at(0), at(1), at(2)], amb: [at(3), at(4), at(5)] };
}
