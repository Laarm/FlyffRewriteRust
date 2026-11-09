// Window move and resize arithmetic (Plan/42 §5.4-§5.5; WndBase.cpp:1158-1260, 1967-2000): pure functions so the
// goldens of the acceptance criteria run without any window.

import { type Point, type Rect, rectHeight, rectWidth } from '../layout/geometry';

/** Edge snap distance of moves and resizes (`CLING`, always on). */
export const SNAP = 10;
/** Resize deltas are quantised to this many pixels. */
export const RESIZE_QUANTUM = 16;
/** Width of the resize border (the window rect deflated by this many px gives the inner rect). */
export const RESIZE_BORDER = 6;

/**
 * The new top-left of a moved window (`pt = pointer - grabOffset`, then the layout-rect cling): within 10 px
 * INSIDE an edge the window snaps flush to it, beyond an edge it is clamped back inside. `L` is the root layout
 * rect (the viewport minus the taskbar); a window larger than `L` ends up pinned to the right/bottom clamp
 * exactly like the original's sequence of `if`s.
 */
export function snapMove(topLeft: Point, w: number, h: number, L: Rect): Point {
  let x = topLeft.x;
  let y = topLeft.y;
  if (x < L.l + SNAP && x > L.l) x = L.l;
  if (y < L.t + SNAP && y > L.t) y = L.t;
  if (x + w > L.r - SNAP && x + w < L.r) x = L.r - w;
  if (y + h > L.b - SNAP && y + h < L.b) y = L.b - h;
  if (x < L.l) x = L.l;
  if (y < L.t) y = L.t;
  if (x + w > L.r) x = L.r - w;
  if (y + h > L.b) y = L.b - h;
  return { x, y };
}

/** C integer division by 16 toward zero, times 16 (`pt = (pt / 16) * 16`): 23 -> 16, -23 -> -16, 15 -> 0. */
export function quantize(d: number): number {
  return Math.trunc(d / RESIZE_QUANTUM) * RESIZE_QUANTUM;
}

/**
 * The resize zone under a point given relative to the window's top-left (`GetResizeDir`): 0 none (inside the inner
 * rect or outside the window), 1 top, 2 bottom, 3 left, 4 right, 5 top-left, 6 top-right, 7 bottom-left,
 * 8 bottom-right. The inner rect is the window rect deflated by 6 px; corner checks use strict comparisons.
 */
export function resizeDir(pt: Point, width: number, height: number): number {
  if (pt.x < 0 || pt.x >= width || pt.y < 0 || pt.y >= height) return 0;
  const l = RESIZE_BORDER;
  const t = RESIZE_BORDER;
  const r = width - RESIZE_BORDER;
  const b = height - RESIZE_BORDER;
  if (pt.x < l && pt.y < t) return 5;
  if (pt.x > r && pt.y < t) return 6;
  if (pt.x < l && pt.y > b) return 7;
  if (pt.x > r && pt.y > b) return 8;
  if (pt.y < t) return 1;
  if (pt.y > b) return 2;
  if (pt.x < l) return 3;
  if (pt.x > r) return 4;
  return 0;
}

/**
 * The window rect after a resize drag: the edges of `start` named by `dir` move by the quantised pointer delta
 * `d` (`pointer - pointerAtStart`), then ALL four edges snap to the layout rect when within 10 px of it
 * (`top < L.t + 10 -> L.t`, `bottom > L.b - 10 -> L.b`, `left < L.l + 10 -> L.l`, `right > L.r - 10 -> L.r`).
 * KEEP: the snap also moves edges the drag did not touch, like the original.
 */
export function resizeRect(dir: number, start: Rect, d: Point, L: Rect): Rect {
  const dx = quantize(d.x);
  const dy = quantize(d.y);
  const r = { ...start };
  switch (dir) {
    case 1:
      r.t = start.t + dy;
      break;
    case 2:
      r.b = start.b + dy;
      break;
    case 3:
      r.l = start.l + dx;
      break;
    case 4:
      r.r = start.r + dx;
      break;
    case 5:
      r.l = start.l + dx;
      r.t = start.t + dy;
      break;
    case 6:
      r.r = start.r + dx;
      r.t = start.t + dy;
      break;
    case 7:
      r.l = start.l + dx;
      r.b = start.b + dy;
      break;
    case 8:
      r.r = start.r + dx;
      r.b = start.b + dy;
      break;
    default:
      break;
  }
  if (r.t < L.t + SNAP) r.t = L.t;
  if (r.b > L.b - SNAP) r.b = L.b;
  if (r.l < L.l + SNAP) r.l = L.l;
  if (r.r > L.r - SNAP) r.r = L.r;
  return r;
}

/**
 * `CWndBase::AdjustMinRect` (WndBase.cpp:2480-2500): enforces a minimum size by moving the edge the drag moved
 * (the left edge when it is right of the old left, else the right edge; same for top/bottom). Controllers with a
 * `THICKFRAME` frame call it from their `constrain` hook.
 */
export function adjustMinRect(r: Rect, old: Rect, minW: number, minH: number): Rect {
  const out = { ...r };
  if (rectWidth(out) < minW) {
    if (out.l > old.l) out.l = out.r - minW;
    else out.r = out.l + minW;
  }
  if (rectHeight(out) < minH) {
    if (out.t > old.t) out.t = out.b - minH;
    else out.b = out.t + minH;
  }
  return out;
}

/** `CWndBase::AdjustMaxRect` (WndBase.cpp:2501-2523), the mirror image of {@link adjustMinRect}. */
export function adjustMaxRect(r: Rect, old: Rect, maxW: number, maxH: number): Rect {
  const out = { ...r };
  if (rectWidth(out) > maxW) {
    if (out.l < old.l) out.l = out.r - maxW;
    else out.r = out.l + maxW;
  }
  if (rectHeight(out) > maxH) {
    if (out.t < old.t) out.t = out.b - maxH;
    else out.b = out.t + maxH;
  }
  return out;
}
