// The flight HUD of the world overlay (Plan/41 section 19.7, WP-REND-25): the red rectangles around the movers of the flight-target list
// (`WndWorld.cpp:1118-1133`: `RenderRect(bound rect, ARGB(0xff, 255, 32, 32))` for every mover of `m_aFlyTarget`), and the arrow at the
// edge of the screen that points to a focus the camera does not show.

import type { ObjId, ScreenRect } from '../types';
import type { ScreenPos } from './anchor';
import type { PolyItem, RectItem } from './types';

/** `D3DCOLOR_ARGB(0xff, 255, 32, 32)`. */
export const FLY_TARGET_COLOR = 0xffff2020;

/** One red outline per fly target that has a bound rect this frame (a target that is off the screen has none). */
export function layoutFlyTargetRects(
  targets: readonly ObjId[],
  boundsOf: (id: ObjId) => ScreenRect | null,
): RectItem[] {
  const out: RectItem[] = [];
  for (const id of targets) {
    const r = boundsOf(id);
    if (r) {
      out.push({
        kind: 'rect',
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
        color: FLY_TARGET_COLOR,
        filled: false,
      });
    }
  }
  return out;
}

/** Distance of the arrow tip from the edge of the view (px) and the size of the arrow head. */
export const ARROW_MARGIN = 36;
export const ARROW_LENGTH = 26;
export const ARROW_WIDTH = 18;
/** `ARGB(0xc0, 255, 64, 64)`. */
export const ARROW_COLOR = 0xc0ff4040;

export interface FocusArrowInput {
  /** The size of the world view in CSS pixels. */
  readonly width: number;
  readonly height: number;
  /** The screen position of the focus, `null` when it is behind the camera. */
  readonly target: ScreenPos | null;
}

/** Whether a projected point is inside the view (the margin keeps an arrow from flickering at the edge). */
function onScreen(t: ScreenPos, w: number, h: number): boolean {
  return t.x >= 0 && t.x <= w && t.y >= 0 && t.y <= h;
}

/**
 * The off-screen focus arrow of a flying player: a triangle at the edge of the view whose tip points from the centre towards the
 * focus. Nothing when the focus is on the screen (the brackets show it); a focus behind the camera points down (turn around).
 */
export function layoutFocusArrow(i: FocusArrowInput): PolyItem | null {
  const { width: w, height: h, target } = i;
  if (!(w > 0 && h > 0)) return null;
  if (target !== null && onScreen(target, w, h)) return null;
  const cx = w / 2;
  const cy = h / 2;
  let dx = 0;
  let dy = 1;
  if (target !== null) {
    dx = target.x - cx;
    dy = target.y - cy;
    const len = Math.hypot(dx, dy);
    if (len < 1e-6) return null;
    dx /= len;
    dy /= len;
  }
  // the tip: the ray from the centre meets the inner rectangle of the view
  const hw = cx - ARROW_MARGIN;
  const hh = cy - ARROW_MARGIN;
  const tx = Math.abs(dx) < 1e-9 ? Number.POSITIVE_INFINITY : hw / Math.abs(dx);
  const ty = Math.abs(dy) < 1e-9 ? Number.POSITIVE_INFINITY : hh / Math.abs(dy);
  const t = Math.min(tx, ty);
  const tipX = cx + dx * t;
  const tipY = cy + dy * t;
  const baseX = tipX - dx * ARROW_LENGTH;
  const baseY = tipY - dy * ARROW_LENGTH;
  const px = -dy * (ARROW_WIDTH / 2);
  const py = dx * (ARROW_WIDTH / 2);
  return {
    kind: 'poly',
    points: [tipX, tipY, baseX + px, baseY + py, baseX - px, baseY - py].map(Math.round),
    color: ARROW_COLOR,
  };
}
