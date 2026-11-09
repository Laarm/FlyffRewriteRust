// Small CPU image helpers of the terrain texture pool (Plan/41 section 9.4): textures that cannot sit in a natural
// bucket of the pool (no mip chain, an odd shape, a combination beyond the six buckets) are decoded to RGBA8,
// resampled to the fallback size and given a generated 4-level chain.

/** One level of an RGBA8 chain. */
export interface RgbaLevel {
  readonly w: number;
  readonly h: number;
  readonly data: Uint8Array;
}

/**
 * Resamples RGBA8 `src` (`sw` x `sh`) to `dw` x `dh` with bilinear filtering at texel centres. Downscaling by an
 * integer factor of 2 is exactly the 2 x 2 box filter; upscaling interpolates. Equal sizes copy.
 */
export function resampleRgba8(
  src: Uint8Array,
  sw: number,
  sh: number,
  dw: number,
  dh: number,
): Uint8Array {
  const out = new Uint8Array(dw * dh * 4);
  if (sw === dw && sh === dh) {
    out.set(src.subarray(0, dw * dh * 4));
    return out;
  }
  const kx = sw / dw;
  const ky = sh / dh;
  for (let y = 0; y < dh; y++) {
    const fy = (y + 0.5) * ky - 0.5;
    const y0 = Math.max(0, Math.min(sh - 1, Math.floor(fy)));
    const y1 = Math.min(sh - 1, y0 + 1);
    const ty = Math.min(1, Math.max(0, fy - y0));
    for (let x = 0; x < dw; x++) {
      const fx = (x + 0.5) * kx - 0.5;
      const x0 = Math.max(0, Math.min(sw - 1, Math.floor(fx)));
      const x1 = Math.min(sw - 1, x0 + 1);
      const tx = Math.min(1, Math.max(0, fx - x0));
      for (let c = 0; c < 4; c++) {
        const a = src[(y0 * sw + x0) * 4 + c] ?? 0;
        const b = src[(y0 * sw + x1) * 4 + c] ?? 0;
        const d = src[(y1 * sw + x0) * 4 + c] ?? 0;
        const e = src[(y1 * sw + x1) * 4 + c] ?? 0;
        const top = a + (b - a) * tx;
        const bottom = d + (e - d) * tx;
        out[(y * dw + x) * 4 + c] = Math.round(top + (bottom - top) * ty);
      }
    }
  }
  return out;
}

/** The next mip level: the 2 x 2 average of every block (dimensions halve, minimum 1). */
export function downsample2x(src: Uint8Array, w: number, h: number): RgbaLevel {
  const dw = Math.max(1, w >> 1);
  const dh = Math.max(1, h >> 1);
  const out = new Uint8Array(dw * dh * 4);
  for (let y = 0; y < dh; y++) {
    const y0 = Math.min(h - 1, y * 2);
    const y1 = Math.min(h - 1, y * 2 + 1);
    for (let x = 0; x < dw; x++) {
      const x0 = Math.min(w - 1, x * 2);
      const x1 = Math.min(w - 1, x * 2 + 1);
      for (let c = 0; c < 4; c++) {
        const s =
          (src[(y0 * w + x0) * 4 + c] ?? 0) +
          (src[(y0 * w + x1) * 4 + c] ?? 0) +
          (src[(y1 * w + x0) * 4 + c] ?? 0) +
          (src[(y1 * w + x1) * 4 + c] ?? 0);
        out[(y * dw + x) * 4 + c] = (s + 2) >> 2;
      }
    }
  }
  return { w: dw, h: dh, data: out };
}

/** `levels` levels starting at `level0` (`w` x `h`), each the box filter of the previous one. */
export function buildChain(level0: Uint8Array, w: number, h: number, levels: number): RgbaLevel[] {
  const chain: RgbaLevel[] = [{ w, h, data: level0 }];
  for (let i = 1; i < levels; i++) {
    const prev = chain[i - 1];
    if (!prev) break;
    chain.push(downsample2x(prev.data, prev.w, prev.h));
  }
  return chain;
}
