// The sprite sheet of the entity layer (Plan/25 section 3.5): one canvas of 32 px cells, drawn once. The shapes are white (or grey) with a dark
// outline so that the per-instance colour of the renderer tints them; the arrow and the selection pulse are the game's navigator pictures
// (`ImgNavArrow`, `ImgNavPos`, loaded through the asset URLs, `gameAssets.ts`) when they arrived, and drawn shapes otherwise.
import { Sprite } from './style';

/** Pixels per cell. */
export const CELL = 32;
export const ATLAS_COLUMNS = 8;
export const ATLAS_ROWS = 2;
export const ATLAS_W = CELL * ATLAS_COLUMNS;
export const ATLAS_H = CELL * ATLAS_ROWS;

/** `[u0, v0, u1, v1]` of a cell in texture coordinates, row 0 on top (the texture is uploaded without a flip). */
export function cellUv(id: number): readonly [number, number, number, number] {
  const col = id % ATLAS_COLUMNS;
  const row = Math.floor(id / ATLAS_COLUMNS);
  // half a texel inside, so linear filtering never reads the neighbour cell
  const e = 0.5;
  return [
    (col * CELL + e) / ATLAS_W,
    (row * CELL + e) / ATLAS_H,
    ((col + 1) * CELL - e) / ATLAS_W,
    ((row + 1) * CELL - e) / ATLAS_H,
  ];
}

/** The pictures of the game that replace drawn shapes. */
export interface GamePictures {
  /** `ImgNavArrow` (32 x 32, transparent background, points north). */
  readonly arrow?: CanvasImageSource | undefined;
  /** `ImgNavPos` (128 x 32: four frames of 32 x 32). */
  readonly pos?: CanvasImageSource | undefined;
  /** The rectangles `[x, y, w, h]` of its frames from `NavPosition.inc` of the `ui-atlases` bundle; four 32 px frames side by side when absent. */
  readonly posFrames?: readonly (readonly [number, number, number, number])[] | undefined;
}

const OUTLINE = 'rgba(8, 12, 20, 0.85)';

type Ctx = CanvasRenderingContext2D;

function cellOrigin(id: number): [number, number] {
  return [(id % ATLAS_COLUMNS) * CELL + CELL / 2, Math.floor(id / ATLAS_COLUMNS) * CELL + CELL / 2];
}

function stroked(c: Ctx, path: () => void, fill: boolean, width = 2): void {
  c.beginPath();
  path();
  c.lineJoin = 'round';
  c.lineWidth = width + 2;
  c.strokeStyle = OUTLINE;
  c.stroke();
  c.lineWidth = width;
  c.strokeStyle = '#fff';
  if (fill) {
    c.fillStyle = '#fff';
    c.fill();
  }
  c.stroke();
}

function drawArrow(c: Ctx, cx: number, cy: number): void {
  stroked(
    c,
    () => {
      c.moveTo(cx, cy - 12);
      c.lineTo(cx + 8, cy + 11);
      c.lineTo(cx, cy + 6);
      c.lineTo(cx - 8, cy + 11);
      c.closePath();
    },
    true,
    1,
  );
}

function drawDot(c: Ctx, cx: number, cy: number): void {
  stroked(
    c,
    () => {
      c.arc(cx, cy, 8, 0, Math.PI * 2);
    },
    true,
    1,
  );
}

function drawRing(c: Ctx, cx: number, cy: number, radius: number, dash: boolean): void {
  c.beginPath();
  c.arc(cx, cy, radius, 0, Math.PI * 2);
  c.lineWidth = 5;
  c.strokeStyle = OUTLINE;
  c.setLineDash(dash ? [4, 3] : []);
  c.stroke();
  c.lineWidth = 3;
  c.strokeStyle = '#fff';
  c.stroke();
  c.setLineDash([]);
}

function drawCross(c: Ctx, cx: number, cy: number): void {
  const r = 8;
  c.lineCap = 'round';
  for (const [w, s] of [
    [6, OUTLINE],
    [3.5, '#fff'],
  ] as const) {
    c.lineWidth = w;
    c.strokeStyle = s;
    c.beginPath();
    c.moveTo(cx - r, cy - r);
    c.lineTo(cx + r, cy + r);
    c.moveTo(cx + r, cy - r);
    c.lineTo(cx - r, cy + r);
    c.stroke();
  }
}

function drawStar(c: Ctx, cx: number, cy: number): void {
  stroked(
    c,
    () => {
      for (let i = 0; i < 10; i += 1) {
        const r = i % 2 === 0 ? 14 : 6;
        const a = -Math.PI / 2 + (i * Math.PI) / 5;
        const x = cx + Math.cos(a) * r;
        const y = cy + Math.sin(a) * r;
        if (i === 0) c.moveTo(x, y);
        else c.lineTo(x, y);
      }
      c.closePath();
    },
    true,
    1,
  );
}

function drawBlob(c: Ctx, cx: number, cy: number): void {
  const g = c.createRadialGradient(cx, cy, 0, cx, cy, CELL / 2);
  g.addColorStop(0, 'rgba(255,255,255,1)');
  g.addColorStop(1, 'rgba(255,255,255,0)');
  c.fillStyle = g;
  c.fillRect(cx - CELL / 2, cy - CELL / 2, CELL, CELL);
}

/** Draws the sheet. `null` outside a browser (no 2D canvas). */
export function buildAtlas(pictures: GamePictures = {}): HTMLCanvasElement | null {
  if (typeof document === 'undefined') return null;
  const canvas = document.createElement('canvas');
  canvas.width = ATLAS_W;
  canvas.height = ATLAS_H;
  const c = canvas.getContext('2d');
  if (!c) return null;
  c.clearRect(0, 0, ATLAS_W, ATLAS_H);
  const at = (id: number): [number, number] => cellOrigin(id);
  const [ax, ay] = at(Sprite.ARROW);
  if (pictures.arrow) c.drawImage(pictures.arrow, ax - CELL / 2, ay - CELL / 2, CELL, CELL);
  else drawArrow(c, ax, ay);
  const [dx, dy] = at(Sprite.DOT);
  drawDot(c, dx, dy);
  const [rx, ry] = at(Sprite.RING);
  drawRing(c, rx, ry, 12, false);
  const [qx, qy] = at(Sprite.DASH_RING);
  drawRing(c, qx, qy, 12, true);
  const [xx, xy] = at(Sprite.CROSS);
  drawCross(c, xx, xy);
  const [sx, sy] = at(Sprite.SQUARE);
  stroked(
    c,
    () => {
      c.rect(sx - 7, sy - 7, 14, 14);
    },
    true,
    1,
  );
  const [mx, my] = at(Sprite.DIAMOND);
  stroked(
    c,
    () => {
      c.moveTo(mx, my - 10);
      c.lineTo(mx + 9, my);
      c.lineTo(mx, my + 10);
      c.lineTo(mx - 9, my);
      c.closePath();
    },
    true,
    1,
  );
  const [tx, ty] = at(Sprite.STAR);
  drawStar(c, tx, ty);
  for (let i = 0; i < 4; i += 1) {
    const id = Sprite.POS0 + i;
    const [px, py] = at(id);
    if (pictures.pos) {
      const [sx, sy, sw, sh] = pictures.posFrames?.[i] ?? [i * CELL, 0, CELL, CELL];
      // the frame is drawn at its own size in the middle of the cell (the pulse is 32 x 32 in the original)
      const k = Math.min(1, CELL / Math.max(sw, sh));
      c.drawImage(
        pictures.pos,
        sx,
        sy,
        sw,
        sh,
        px - (sw * k) / 2,
        py - (sh * k) / 2,
        sw * k,
        sh * k,
      );
    } else {
      // four growing rings: the pulse under a selected entity
      drawRing(c, px, py, 8 + i * 2, false);
    }
  }
  const [bx, by] = at(Sprite.BLOB);
  drawBlob(c, bx, by);
  return canvas;
}
