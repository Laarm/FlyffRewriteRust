// RGBA pixel buffers and the skin compositing rule (Plan/42 §7.3; `PaintTexture`, `_Common/vutil.cpp:250+`).
//
// The skin baker never uses `drawImage` (its blending differs from the original): tiles are decoded to
// straight-alpha RGBA bytes and copied with an OVERWRITE rule that skips keyed pixels. Everything in this file
// is pure and runs in Node, so the baking geometry is unit-testable without a canvas.

/** A decoded image: `width * height * 4` RGBA bytes, straight (non-premultiplied) alpha, row 0 at the top. */
export interface PixelImage {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
  /**
   * `true` when the source had an alpha channel: pixels with `a < 16` are skipped. `false` for sources without
   * one (24-bit BMP): pixels whose top nibbles are `(F, 0, F)` (magenta) are skipped. Images decoded from the
   * server's `.nk.png` form are always `true` (the server already turned keyed pixels into alpha 0).
   */
  readonly hasAlpha: boolean;
}

/** A writable skin target (the baked frame canvas contents). */
export interface PixelBuffer {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
}

/** The 4-bit alpha cut of the 32-bit rule: `a >> 4 != 0` writes, i.e. `a < 16` is skipped. */
export const ALPHA_KEY_BELOW = 16;

/** A transparent buffer. */
export function createBuffer(width: number, height: number): PixelBuffer {
  return { width, height, data: new Uint8ClampedArray(width * height * 4) };
}

/** An image from RGBA bytes (copied when `copy`). */
export function imageFromRgba(
  width: number,
  height: number,
  rgba: ArrayLike<number>,
  hasAlpha = true,
): PixelImage {
  const data = new Uint8ClampedArray(width * height * 4);
  data.set(rgba);
  return { width, height, data, hasAlpha };
}

/** A single-colour image (tests, placeholders). */
export function solidImage(
  width: number,
  height: number,
  rgba: readonly [number, number, number, number],
  hasAlpha = true,
): PixelImage {
  const data = new Uint8ClampedArray(width * height * 4);
  for (let i = 0; i < width * height; i += 1) {
    data[i * 4] = rgba[0];
    data[i * 4 + 1] = rgba[1];
    data[i * 4 + 2] = rgba[2];
    data[i * 4 + 3] = rgba[3];
  }
  return { width, height, data, hasAlpha };
}

/** RGBA of one pixel (`[0, 0, 0, 0]` outside the buffer). */
export function pixelAt(
  buf: PixelBuffer | PixelImage,
  x: number,
  y: number,
): [number, number, number, number] {
  if (x < 0 || y < 0 || x >= buf.width || y >= buf.height) return [0, 0, 0, 0];
  const i = (y * buf.width + x) * 4;
  const d = buf.data;
  return [d[i] ?? 0, d[i + 1] ?? 0, d[i + 2] ?? 0, d[i + 3] ?? 0];
}

/** `true` when the pixel is skipped by the key rule of `hasAlpha` (the original's `PaintTexture` conditions). */
export function isKeyed(r: number, g: number, b: number, a: number, hasAlpha: boolean): boolean {
  return hasAlpha ? a < ALPHA_KEY_BELOW : r >> 4 === 0xf && g >> 4 === 0 && b >> 4 === 0xf;
}

/**
 * Copies `src` into `dst` at `(x, y)` with the `PaintTexture` rule: keyed pixels leave the destination as it
 * is, every other pixel REPLACES the destination pixel (no blending). Sources without alpha write alpha 255.
 * The copy is clipped to the destination; nothing outside it is written (the original's linear offset check let
 * a line wrap around, which is not reproduced).
 *
 * @param srcRect optional sub-rectangle of the source `[sx, sy, sw, sh]` (atlas sprites)
 */
export function blit(
  dst: PixelBuffer,
  src: PixelImage,
  x: number,
  y: number,
  srcRect?: readonly [number, number, number, number],
): void {
  const sx0 = srcRect ? srcRect[0] : 0;
  const sy0 = srcRect ? srcRect[1] : 0;
  const sw = srcRect ? srcRect[2] : src.width;
  const sh = srcRect ? srcRect[3] : src.height;
  // clip the source rectangle to the image and the destination
  const colStart = Math.max(0, -x, -sx0);
  const rowStart = Math.max(0, -y, -sy0);
  const colEnd = Math.min(sw, dst.width - x, src.width - sx0);
  const rowEnd = Math.min(sh, dst.height - y, src.height - sy0);
  if (colEnd <= colStart || rowEnd <= rowStart) return;
  const s = src.data;
  const d = dst.data;
  const hasAlpha = src.hasAlpha;
  for (let row = rowStart; row < rowEnd; row += 1) {
    let si = ((sy0 + row) * src.width + sx0 + colStart) * 4;
    let di = ((y + row) * dst.width + x + colStart) * 4;
    for (let col = colStart; col < colEnd; col += 1, si += 4, di += 4) {
      const r = s[si] ?? 0;
      const g = s[si + 1] ?? 0;
      const b = s[si + 2] ?? 0;
      const a = s[si + 3] ?? 0;
      if (hasAlpha ? a < ALPHA_KEY_BELOW : r >> 4 === 0xf && g >> 4 === 0 && b >> 4 === 0xf) {
        continue;
      }
      d[di] = r;
      d[di + 1] = g;
      d[di + 2] = b;
      d[di + 3] = hasAlpha ? a : 255;
    }
  }
}

/**
 * Draws `src` over the whole `w x h` rectangle at `(x, y)` scaled with nearest-neighbour sampling, using the same
 * key rule as {@link blit} (used by the atlas colour-key pass and by stretched backgrounds).
 */
export function blitScaled(
  dst: PixelBuffer,
  src: PixelImage,
  x: number,
  y: number,
  w: number,
  h: number,
): void {
  if (w <= 0 || h <= 0 || src.width === 0 || src.height === 0) return;
  for (let row = 0; row < h; row += 1) {
    const dy = y + row;
    if (dy < 0 || dy >= dst.height) continue;
    const sy = Math.min(src.height - 1, Math.floor((row * src.height) / h));
    for (let col = 0; col < w; col += 1) {
      const dx = x + col;
      if (dx < 0 || dx >= dst.width) continue;
      const sx = Math.min(src.width - 1, Math.floor((col * src.width) / w));
      const si = (sy * src.width + sx) * 4;
      const r = src.data[si] ?? 0;
      const g = src.data[si + 1] ?? 0;
      const b = src.data[si + 2] ?? 0;
      const a = src.data[si + 3] ?? 0;
      if (isKeyed(r, g, b, a, src.hasAlpha)) continue;
      const di = (dy * dst.width + dx) * 4;
      dst.data[di] = r;
      dst.data[di + 1] = g;
      dst.data[di + 2] = b;
      dst.data[di + 3] = src.hasAlpha ? a : 255;
    }
  }
}

/** Fills a rectangle with one colour (clipped; no blending). */
export function fillRect(
  dst: PixelBuffer,
  l: number,
  t: number,
  r: number,
  b: number,
  rgba: readonly [number, number, number, number],
): void {
  const x0 = Math.max(0, l);
  const y0 = Math.max(0, t);
  const x1 = Math.min(dst.width, r);
  const y1 = Math.min(dst.height, b);
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      const i = (y * dst.width + x) * 4;
      dst.data[i] = rgba[0];
      dst.data[i + 1] = rgba[1];
      dst.data[i + 2] = rgba[2];
      dst.data[i + 3] = rgba[3];
    }
  }
}

/** Copies the whole image into a new buffer of the same size (the baker's atlas key pass uses it). */
export function cloneImage(src: PixelImage): PixelImage {
  return { width: src.width, height: src.height, data: src.data.slice(), hasAlpha: src.hasAlpha };
}

/** Approximate byte size of an image (the LRU accounting of the image cache). */
export function imageBytes(src: PixelImage): number {
  return src.data.byteLength;
}
