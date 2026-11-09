// Canvas helpers of the taskbar slot rows: a decoded `PixelImage` drawn at a size, with a per-image offscreen cache.

import { RADAR_FRAMES, radarAngleDeg, radarIndex } from '../icons/anim';
import type { PixelImage } from '../skin/pixels';

const sources = new WeakMap<PixelImage, HTMLCanvasElement>();

/** The image as a canvas (cached); `null` outside a browser or without canvas support. */
export function imageCanvas(img: PixelImage): HTMLCanvasElement | null {
  if (typeof document === 'undefined') return null;
  const cached = sources.get(img);
  if (cached) return cached;
  const c = document.createElement('canvas');
  c.width = img.width;
  c.height = img.height;
  const g = c.getContext('2d');
  if (!g) return null;
  g.putImageData(new ImageData(new Uint8ClampedArray(img.data), img.width, img.height), 0, 0);
  sources.set(img, c);
  return c;
}

/** Draws `img` at `(x, y)` scaled to `w x h` with point sampling (the original drew icons unfiltered). */
export function drawImageScaled(
  ctx: CanvasRenderingContext2D,
  img: PixelImage,
  x: number,
  y: number,
  w: number,
  h: number,
): void {
  const c = imageCanvas(img);
  if (!c) return;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(c, x, y, w, h);
}

/**
 * The slices of a taskbar layout bitmap that carry the width: the `tail` is the right end (frame cap, logo) and the `fill` columns just
 * left of it are flat, so they can be repeated to widen the bar. Measured on `Theme/WndTaskBar<w>.bmp`: the queue frame ends at
 * 778 / 938 / 1194 and a flat gap of 3 / 5 / 5 columns follows it; the 800 layout then has a 13 px cap, the others a 75 px tail
 * (the wide bitmaps only add more flat bar in front of that tail, the left 1200 px are the same in all of them).
 */
export function taskbarBackgroundSlices(legacyWidth: number): { tail: number; fill: number } {
  return legacyWidth <= 800 ? { tail: 13, fill: 3 } : { tail: 75, fill: 5 };
}

/**
 * Paints the layout bitmap of the taskbar into `buffer` (the full virtual width x 48).
 *
 * The slot rows sit at the fixed coordinates of the layout (Plan/42 §11.1), and the bitmap is drawn for exactly those, so it is
 * placed at its native size from the left edge. A stretch to the virtual width (what the original's `RenderTexture` of the full rect did
 * at its fixed screen sizes) moves the painted slot frames away from the icons at every width that is not the layout's own. A wider
 * buffer keeps the `tail` slice at its right edge and repeats the `fill` slice in between; a narrower one cuts the right end off.
 */
export function paintTaskbarBackground(
  buffer: { width: number; height: number; data: Uint8ClampedArray },
  src: PixelImage,
  slices: { tail: number; fill: number },
): void {
  const { width: w, height: h, data } = buffer;
  const sw = src.width;
  const tail = Math.min(slices.tail, sw);
  const fill = Math.max(1, Math.min(slices.fill, sw - tail));
  const body = sw - tail;
  for (let x = 0; x < w; x += 1) {
    let sx: number;
    if (w <= sw || x < body) sx = x;
    else if (x >= w - tail) sx = body + (x - (w - tail));
    else sx = body - fill + ((x - body) % fill);
    sx = Math.min(sx, sw - 1);
    for (let y = 0; y < h; y += 1) {
      const sy = Math.min(src.height - 1, Math.floor((y * src.height) / h));
      const si = (sy * sw + sx) * 4;
      const di = (y * w + x) * 4;
      data[di] = src.data[si] ?? 0;
      data[di + 1] = src.data[si + 1] ?? 0;
      data[di + 2] = src.data[si + 2] ?? 0;
      data[di + 3] = src.data[si + 3] ?? 0;
    }
  }
}

/**
 * The cool-down radar on a 32 x 32 icon (`RenderRadar`, WndManager.cpp:146-229): a white square at alpha 120 and, over it, the part of
 * the clock face that is still covered (alpha 200). `elapsed` of `total` ms are over; the hand angle of frame `MulDiv(elapsed, 127,
 * total)` is `radarAngleDeg` (the area rule of the quadrant images). The icon sits one pixel lower than the cell (`pt.y += 1`).
 */
export function drawRadar(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  elapsed: number,
  total: number,
): void {
  if (!(total > 0)) return;
  const idx = radarIndex(Math.max(0, elapsed), total);
  if (idx < 0 || idx > RADAR_FRAMES) return;
  const top = y + 1;
  ctx.save();
  ctx.fillStyle = 'rgba(255, 255, 255, 0.4706)';
  ctx.fillRect(x, top, 32, 32);
  const angle = radarAngleDeg(Math.min(idx, RADAR_FRAMES - 1));
  if (idx < RADAR_FRAMES) {
    // the still covered part: from the hand clockwise to the full turn, clipped to the icon square
    ctx.beginPath();
    ctx.rect(x, top, 32, 32);
    ctx.clip();
    const cx = x + 16;
    const cy = top + 16;
    const from = ((angle - 90) * Math.PI) / 180;
    ctx.beginPath();
    ctx.moveTo(cx, cy);
    ctx.arc(cx, cy, 48, from, (270 * Math.PI) / 180, false);
    ctx.closePath();
    ctx.fillStyle = 'rgba(255, 255, 255, 0.7843)';
    ctx.fill();
  }
  ctx.restore();
}
