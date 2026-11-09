// The tooltip frame (`CToolTip::Paint`, tooltip.cpp:398-645; Plan/42 §9.2): the nine tiles `Theme/WndTooltipTile00-08.tga`
// (16 x 16, one set) drawn around the text rect inflated by 8 px. Edge and centre tiles repeat on the 16 px grid; the LAST
// row and column are stretched (or squeezed) to reach the right / bottom corner tiles; the four corners are drawn last.
// `tooltipFrame(w, h)` is the pure draw list (goldens in the tests), `paintTooltipFrame` runs it on a canvas.

import type { PixelImage } from './pixels';

/** One tile to draw: tile index 0-8 (0 1 2 / 3 4 5 / 6 7 8), the box in px from the TEXT rect's top-left. */
export interface FrameOp {
  tile: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface TooltipFrame {
  ops: FrameOp[];
  /** The frame's bounding box from the text rect's top-left: `(-8, -8)` and the text size + 16. */
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The size of a tile image. */
export const TOOLTIP_TILE = 16;

/**
 * The draw list for a text rect of `w x h` px (`m_rectRender`): rows `max(1, floor((h + 16) / 16))`, columns
 * `max(1, floor((w + 16) / 16))`; row 0 / column 0 hold the edge tiles (1 / 7 top and bottom, 3 / 5 left and right), the
 * inner cells tile 4; the last row and column are stretched to the corner tiles at `P.right` / `P.bottom`.
 */
export function tooltipFrame(w: number, h: number): TooltipFrame {
  const rows = Math.max(h + 16 > 0 ? 1 : 0, Math.floor((h + 16) / 16));
  const cols = Math.max(w + 16 > 0 ? 1 : 0, Math.floor((w + 16) / 16));
  // P = the text rect inflated by 8 on the left/top; its right/bottom are the corner tile positions
  const pl = -8;
  const pt = -8;
  const pr = w - 16 + 8;
  const pb = h - 16 + 8;
  const ops: FrameOp[] = [];
  for (let i = 0; i < rows; i += 1) {
    for (let j = 0; j < cols; j += 1) {
      if (i === 0 || j === 0) {
        if (j === 0 && i !== 0) {
          const last = i + 1 === rows;
          const hh = last ? pb - (pt + i * 16) : 16;
          ops.push({ tile: 3, x: pl, y: pt + i * 16, w: 16, h: hh });
          ops.push({ tile: 5, x: pr, y: pt + i * 16, w: 16, h: hh });
        } else if (i === 0 && j !== 0) {
          const last = j + 1 === cols;
          const ww = last ? pr - (pl + j * 16) : 16;
          ops.push({ tile: 1, x: pl + j * 16, y: pt, w: ww, h: 16 });
          ops.push({ tile: 7, x: pl + j * 16, y: pb, w: ww, h: 16 });
        }
        continue;
      }
      const lastRow = i === rows - 1;
      const lastCol = j === cols - 1;
      const x = pl + j * 16;
      const y = pt + i * 16;
      ops.push({
        tile: 4,
        x,
        y,
        w: lastCol ? pr - x : 16,
        h: lastRow ? pb - y : 16,
      });
    }
  }
  ops.push({ tile: 0, x: pl, y: pt, w: 16, h: 16 });
  ops.push({ tile: 2, x: pr, y: pt, w: 16, h: 16 });
  ops.push({ tile: 6, x: pl, y: pb, w: 16, h: 16 });
  ops.push({ tile: 8, x: pr, y: pb, w: 16, h: 16 });
  return { ops, left: pl, top: pt, width: w + 16, height: h + 16 };
}

/** The slice of a 2-D canvas context the painter needs. */
export interface FrameContext {
  imageSmoothingEnabled: boolean;
  drawImage(image: CanvasImageSource, dx: number, dy: number, dw: number, dh: number): void;
}

/**
 * Draws the frame: every op scales its tile (linear filter, like the `Render(..., size)` of the original); ops of zero
 * size are skipped. `dx`, `dy` is where the text rect's top-left sits on the canvas.
 */
export function paintTooltipFrame(
  ctx: FrameContext,
  tiles: readonly (CanvasImageSource | null)[],
  frame: TooltipFrame,
  dx: number,
  dy: number,
): void {
  ctx.imageSmoothingEnabled = true;
  for (const op of frame.ops) {
    const tile = tiles[op.tile];
    if (!tile || op.w <= 0 || op.h <= 0) continue;
    ctx.drawImage(tile, dx + op.x, dy + op.y, op.w, op.h);
  }
}

/** A decoded tile as a canvas the context can draw (`null` without a DOM). */
export function tileCanvas(img: PixelImage): CanvasImageSource | null {
  if (typeof document === 'undefined') return null;
  const c = document.createElement('canvas');
  c.width = img.width;
  c.height = img.height;
  const ctx = c.getContext('2d');
  if (!ctx) return null;
  ctx.putImageData(
    new ImageData(img.data as Uint8ClampedArray<ArrayBuffer>, img.width, img.height),
    0,
    0,
  );
  return c;
}
