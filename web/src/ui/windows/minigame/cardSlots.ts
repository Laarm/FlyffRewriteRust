// The slot geometry shared by the two card windows (`HitTest` of `CWndFindWordGame` / `CWndPuzzleGame`): the first slot whose layout rectangle contains a client point.

import { type Point, type Rect, rectContains } from '../../layout/geometry';

/** The index of the rect that contains `pt`, -1 for none. */
export function slotAt(rects: readonly Rect[], pt: Point): number {
  for (let i = 0; i < rects.length; i += 1) {
    const r = rects[i];
    if (r && rectContains(r, pt)) return i;
  }
  return -1;
}

/** A root-window point made relative to the client area (what `OnDropIcon` receives is relative to the window). */
export function clientPoint(ptWindow: Point, client: Rect, window: Rect): Point {
  return { x: ptWindow.x - (client.l - window.l), y: ptWindow.y - (client.t - window.t) };
}
