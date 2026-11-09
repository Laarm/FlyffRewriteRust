// SFX textures keep the A1R5G5B5 look of the original (Plan/41 section 16.1, R9, ref/20 section 4.7): the RGBA8
// result of the BC decode is quantised per level to `RGBA5551` (5-bit colour, 1-bit alpha `a >= 128`) and uploaded
// as `UNSIGNED_SHORT_5_5_5_1`, so the GPU filters 1-bit/5-bit texels like D3D did. The formulas are those of the
// DDS fix builder (Plan/24 section 10.4.2, Plan/30 section 11.6): 5-bit `(v * 31 + 127) / 255` (integer division).

/** 8-bit to 5-bit: `(v * 31 + 127) / 255`, truncating. */
export const to5 = (v: number): number => Math.trunc((v * 31 + 127) / 255);

/** 5-bit to 8-bit expansion `(v5 << 3) | (v5 >> 2)` (what the GPU returns for a 5-bit channel, approximately). */
export const from5 = (v5: number): number => (v5 << 3) | (v5 >> 2);

/** One RGBA texel to the `RGBA5551` word `r5 << 11 | g5 << 6 | b5 << 1 | a1` (the layout of `UNSIGNED_SHORT_5_5_5_1`). */
export function packRgba5551(r: number, g: number, b: number, a: number): number {
  return (to5(r) << 11) | (to5(g) << 6) | (to5(b) << 1) | (a >= 128 ? 1 : 0);
}

/**
 * Quantises an RGBA8 level (4 bytes per texel, top-down) to RGBA5551 words. `out` may be given to avoid an
 * allocation (it must hold `texels` words).
 */
export function quantizeRgba5551(rgba: Uint8Array, out?: Uint16Array): Uint16Array {
  const texels = rgba.length >> 2;
  const dst = out ?? new Uint16Array(texels);
  for (let i = 0, s = 0; i < texels; i++, s += 4) {
    dst[i] = packRgba5551(rgba[s] ?? 0, rgba[s + 1] ?? 0, rgba[s + 2] ?? 0, rgba[s + 3] ?? 0);
  }
  return dst;
}

/** Expands an RGBA5551 word back to RGBA8 (colour `(v5 << 3) | (v5 >> 2)`, alpha 0 or 255), for tests. */
export function unpackRgba5551(word: number): [number, number, number, number] {
  return [
    from5((word >> 11) & 31),
    from5((word >> 6) & 31),
    from5((word >> 1) & 31),
    word & 1 ? 255 : 0,
  ];
}
