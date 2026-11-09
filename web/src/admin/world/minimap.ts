// The minimap inset (Plan/25 section 3.2, "Overlays on the canvas"): the overview tile of the whole world, the viewport rectangle (draggable) and the
// ONLINE PLAYERS AS DOTS taken from the roster stream (`RosterDelta`, every 2 s). It opens no map subscription and draws no entity of the frames.
import { TILE_PX, landMeters } from '../components/mapGeo';
import type { View } from './view';

/** The longest side of the inset in CSS pixels. */
export const MINIMAP_PX = 180;

export interface MinimapGeometry {
  /** The inset size in CSS pixels. */
  readonly w: number;
  readonly h: number;
  /** World units per inset pixel. */
  readonly upp: number;
  readonly extent: readonly [number, number];
}

/** The inset for a world: the whole extent fits 180 px on its longer side. */
export function minimapGeometry(
  extent: readonly [number, number],
  max = MINIMAP_PX,
): MinimapGeometry {
  const upp = Math.max(extent[0], extent[1], 1) / max;
  return {
    w: Math.max(1, Math.round(extent[0] / upp)),
    h: Math.max(1, Math.round(extent[1] / upp)),
    upp,
    extent,
  };
}

/** The inset pixel of a world point (z up in the world, down on the screen). */
export function toMinimap(g: MinimapGeometry, x: number, z: number): { mx: number; my: number } {
  return { mx: x / g.upp, my: g.h - z / g.upp };
}

/** The world point of an inset pixel, clamped to the world. */
export function fromMinimap(g: MinimapGeometry, mx: number, my: number): { x: number; z: number } {
  return {
    x: Math.min(g.extent[0], Math.max(0, mx * g.upp)),
    z: Math.min(g.extent[1], Math.max(0, (g.h - my) * g.upp)),
  };
}

/** The viewport rectangle in inset pixels `[x, y, w, h]`, clipped to the inset. */
export function viewportBox(g: MinimapGeometry, v: View): [number, number, number, number] {
  const hw = (v.w / 2) * v.upp;
  const hh = (v.h / 2) * v.upp;
  const a = toMinimap(g, v.cx - hw, v.cz + hh);
  const b = toMinimap(g, v.cx + hw, v.cz - hh);
  const x0 = Math.max(0, Math.min(g.w, a.mx));
  const y0 = Math.max(0, Math.min(g.h, a.my));
  const x1 = Math.max(0, Math.min(g.w, b.mx));
  const y1 = Math.max(0, Math.min(g.h, b.my));
  return [x0, y0, Math.max(1, x1 - x0), Math.max(1, y1 - y0)];
}

/** The part of the top overview tile that shows the world: `[sx, sy, sw, sh]` in texels of a 256 px tile whose row 0 is the north edge of the tile. */
export function overviewSource(
  extent: readonly [number, number],
  mpu: number,
  maxLevel: number,
): [number, number, number, number] {
  const span = landMeters(mpu) * 2 ** maxLevel;
  return [
    0,
    ((span - extent[1]) / span) * TILE_PX,
    (extent[0] / span) * TILE_PX,
    (extent[1] / span) * TILE_PX,
  ];
}

/** One online player. */
export interface MinimapDot {
  readonly x: number;
  readonly z: number;
  readonly world: number;
}

export interface MinimapColors {
  readonly void: string;
  readonly dot: string;
  readonly edge: string;
  readonly box: string;
  readonly follow: string;
}

/** Draws the inset. `image` is the top overview tile (or `null` while it loads or when the world has none). */
export function drawMinimap(
  ctx: CanvasRenderingContext2D,
  g: MinimapGeometry,
  image: CanvasImageSource | null,
  source: readonly [number, number, number, number] | null,
  dots: readonly MinimapDot[],
  view: View,
  colors: MinimapColors,
  followed: { x: number; z: number } | null,
): void {
  ctx.clearRect(0, 0, g.w, g.h);
  ctx.fillStyle = colors.void;
  ctx.fillRect(0, 0, g.w, g.h);
  if (image && source) {
    try {
      ctx.drawImage(image, source[0], source[1], source[2], source[3], 0, 0, g.w, g.h);
    } catch {
      // a bitmap that was closed meanwhile: the void stands
    }
  }
  for (const d of dots) {
    const { mx, my } = toMinimap(g, d.x, d.z);
    ctx.beginPath();
    ctx.arc(mx, my, 2.2, 0, Math.PI * 2);
    ctx.fillStyle = colors.dot;
    ctx.fill();
    ctx.lineWidth = 1;
    ctx.strokeStyle = colors.edge;
    ctx.stroke();
  }
  if (followed) {
    const { mx, my } = toMinimap(g, followed.x, followed.z);
    ctx.beginPath();
    ctx.arc(mx, my, 5, 0, Math.PI * 2);
    ctx.lineWidth = 2;
    ctx.strokeStyle = colors.follow;
    ctx.stroke();
  }
  const [bx, by, bw, bh] = viewportBox(g, view);
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = colors.box;
  ctx.strokeRect(bx + 0.5, by + 0.5, Math.max(1, bw - 1), Math.max(1, bh - 1));
}
