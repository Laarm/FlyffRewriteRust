// The gamma/bright/contrast ramp of the original client as a 256-entry table (Plan/41 section 4.3, PL-161).
//
// The original sets a device gamma ramp at start-up with bright = 10 (src:Neuz/Neuz.cpp:560, 1778-1797;
// defaults src:_Common/HwOption.cpp:256-258):
//
//   for i in 0..255: f = 255 * pow(i / 256, 1 / gamma); f = f * contrast + bright; f = clamp(f, 0, 255);
//                    ramp[i] = (u16) f << 8
//
// The ramp is 8-bit effective: `out = floor(f)`. Defaults gamma 1.0, bright 10, contrast 1.0 make the whole
// image about +10/255 brighter.
//
// PURE MODULE: no imports at all (eslint/rules.js enforces it), so that `device.ts` (the `R8` LUT texture of
// the final render pass) and 42's `skin/ramp.ts` (the DOM UI and cursors, which the original ramped as well)
// share the exact same table.

/** Number of entries of the ramp. */
export const RAMP_SIZE = 256;

/** The defaults of the original options (`HwOption.cpp:256-258`). */
export const DEFAULT_GAMMA = 1.0;
export const DEFAULT_BRIGHT = 10;
export const DEFAULT_CONTRAST = 1.0;

/**
 * Builds the ramp: `out[i] = floor(clamp(255 * pow(i / 256, 1 / gamma) * contrast + bright, 0, 255))`.
 * Accepts any `gamma > 0`, any integer `bright` and `contrast >= 0` (the options window owns the legal
 * ranges); a non-positive or non-finite gamma falls back to 1.
 */
export function buildRampLut(gamma: number, bright: number, contrast: number): Uint8Array {
  const g = Number.isFinite(gamma) && gamma > 0 ? gamma : 1;
  const c = Number.isFinite(contrast) && contrast >= 0 ? contrast : 1;
  const b = Number.isFinite(bright) ? bright : 0;
  const out = new Uint8Array(RAMP_SIZE);
  for (let i = 0; i < RAMP_SIZE; i++) {
    let f = 255 * Math.pow(i / 256, 1 / g);
    f = f * c + b;
    if (f < 0) f = 0;
    else if (f > 255) f = 255;
    out[i] = Math.floor(f);
  }
  return out;
}
