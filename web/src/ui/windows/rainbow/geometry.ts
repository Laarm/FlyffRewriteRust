// Where the rainbow-race windows open (`src:_Interface/WndRainbowRace.cpp:384-440, 611-675, 703-724`; pure functions of the rects, so the numbers are goldens of the tests).
//
//   race button        48 x 48, `x = root.right - width - 5`, `y = navigator.bottom + 5` while the navigator is open, else `root.top + height + 120`
//   progress window    `x = button.right - width` when `root.right - button.right < width`, else `x = button.left`; `y = button.bottom`, then above the button (`y = button.top - height - 10`)
//                      when `root.bottom - button.bottom < height`. Without a button the window stays where it was created.
//   game information   beside the progress window: `x = progress.left - 10 - width` when `root.right - progress.right < width`, else `progress.right + 10`; `y = progress.top`

import type { Point, Rect } from '../../layout/geometry';
import { rectHeight } from '../../layout/geometry';

/** The position of the race button. */
export function buttonPosition(root: Rect, size: Point, navigatorBottom: number | null): Point {
  const x = root.r - size.x - 5;
  const y = navigatorBottom !== null ? navigatorBottom + 5 : root.t + size.y + 120;
  return { x, y };
}

/** The position of the progress window of `size` under (or above) the button rect. */
export function progressPosition(root: Rect, button: Rect, size: Point): Point {
  let x: number;
  let y: number;
  if (root.r - button.r < size.x) {
    x = button.r - size.x;
    y = button.b;
  } else {
    x = button.l;
    y = button.t + rectHeight(button);
  }
  if (root.b - button.b < size.y) y -= rectHeight(button) + size.y + 10;
  return { x, y };
}

/** The position of the game information window of `size` next to the progress window. */
export function infoPosition(root: Rect, progress: Rect, size: Point): Point {
  if (root.r - progress.r < size.x) return { x: progress.l - (10 + size.x), y: progress.t };
  return { x: progress.r + 10, y: progress.b - rectHeight(progress) };
}
