// Where the popups of `APP_INSTANTMSG` go (`CWndMgr::OpenInstantMsg`, `src:_Interface/WndManager.cpp:7250-7280`): a column of popups grows upwards from the bottom right of the
// layout rect, a full column starts the next one to the left. `n` is the number of popups open when the new one opens.

import type { Point, Rect } from '../../layout/geometry';

/** The size of `APP_INSTANTMSG`. */
export const INSTANT_SIZE = { w: 144, h: 112 } as const;

/**
 * `nNumHeight = layoutH / popupH`, `column = n / nNumHeight + 1`, `row = n % nNumHeight + 1`; the top-left is
 * `( right - w * column, bottom - h * row )`. A layout shorter than one popup counts as one row.
 */
export function instantPosition(
  n: number,
  layout: Rect,
  size: { w: number; h: number } = INSTANT_SIZE,
): Point {
  const rows = Math.max(1, Math.trunc((layout.b - layout.t) / size.h));
  const column = Math.trunc(n / rows) + 1;
  const row = (n % rows) + 1;
  return { x: layout.r - size.w * column, y: layout.b - size.h * row };
}
