// Tooltip box size and placement (`CToolTip::PutToolTip` / `Paint`, tooltip.cpp:127-420; Plan/42 §9.1, §9.3), as pure
// functions so that the goldens of the acceptance criteria run without a screen.

import type { Point, Rect } from '../layout/geometry';

/** `0` top, `1` bottom, `2` left, `3` right of the owner. */
export type TooltipPos = 0 | 1 | 2 | 3;

/** The wrap width of the text (`m_strToolTip.Init(font, CRect(0, 0, 200, 0))`). */
export const TOOLTIP_WRAP = 200;
/** The minimum width of an ultimate-weapon tooltip (room for five gem slots). */
export const ULTIMATE_MIN_WIDTH = 194;
/** The band of an ultimate tooltip above the text. */
export const ULTIMATE_BAND = 36;
/** The gap between a tooltip and its comparison tooltip. */
export const SUB_GAP = 16;

/** The size of the text rect: `(textW + 6) x (textH + 3 + 2 * lines)`. */
export function tooltipBox(textW: number, textH: number, lines: number): { w: number; h: number } {
  return { w: textW + 6, h: textH + 3 + 2 * lines };
}

/** The origin of a tooltip of `box` for an owner rect and position (`CToolTip::Paint`). */
export function tooltipOrigin(pos: TooltipPos, owner: Rect, box: { w: number; h: number }): Point {
  switch (pos) {
    case 0:
      return { x: owner.l, y: owner.t - box.h - 10 };
    case 1:
      return { x: owner.l, y: owner.b + 20 };
    case 2:
      return { x: owner.l - box.w - 2, y: owner.t };
    case 3:
      return { x: owner.r, y: owner.t };
  }
}

/** One tooltip to place: its box and whether it is an ultimate item (the band above the text). */
export interface PlaceBox {
  w: number;
  h: number;
  ultimate?: boolean;
}

/** A placed tooltip: where its text rect sits (the frame is drawn 8 px around it). */
export interface PlacedBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Places the main tooltip and up to two comparison tooltips on `screen`:
 *  * main: from `pos` and the owner rect; the right edge clamp moves it left (`screenW - w - 8` alone, `- 224` with one
 *    comparison, `- 470` with two; KEEP: fixed offsets); `top < screen.top` -> `screen.top + 8`;
 *  * comparison 1: `x = main.x + main.w + 16`, comparison 2: `x = sub1.x + sub1.w + 16`, both with the y of position 0
 *    (their own height above the owner), top-clamped like the main one;
 *  * an ultimate tooltip is at least 194 px wide and its frame reaches 36 px above the origin (the box grows by 36).
 * The returned boxes carry the FINAL size (the minimum width applied).
 */
export function placeTooltips(
  pos: TooltipPos,
  owner: Rect,
  boxes: readonly PlaceBox[],
  screen: Rect,
): PlacedBox[] {
  const sized = boxes.map((b) => ({
    ...b,
    w: b.ultimate === true ? Math.max(b.w, ULTIMATE_MIN_WIDTH) : b.w,
  }));
  const main = sized[0];
  if (!main) return [];
  const subs = sized.slice(1, 3);
  const screenW = screen.r - screen.l;
  const out: PlacedBox[] = [];

  /** The generic top clamp, then the ultimate band (the frame reaches 36 px above, or 8 px below the screen top). */
  const vertical = (y: number, b: PlaceBox): { y: number; extra: number } => {
    const y0 = y < screen.t ? screen.t + 8 : y;
    if (b.ultimate !== true) return { y: y0, extra: 0 };
    const top = y0 - ULTIMATE_BAND;
    return { y: top < screen.t ? screen.t + 8 : top, extra: ULTIMATE_BAND };
  };

  // the main tooltip
  let { x, y } = tooltipOrigin(pos, owner, main);
  const right = x + main.w;
  if (subs.length === 0) {
    if (right + 16 > screen.r) x = screenW - main.w - 8;
  } else if (subs.length === 1) {
    const s1 = subs[0];
    if (s1 && right + s1.w + 32 > screen.r) x = screenW - main.w - 224;
  } else {
    const s1 = subs[0];
    const s2 = subs[1];
    if (s1 && s2 && right + s1.w + s2.w + 48 > screen.r) x = screenW - main.w - 470;
  }
  const mv = vertical(y, main);
  y = mv.y;
  out.push({ x, y, w: main.w, h: main.h + mv.extra });

  // the comparison tooltips
  let prev = out[0] ?? { x, y, w: main.w, h: main.h };
  for (const sub of subs) {
    const o = tooltipOrigin(0, owner, sub);
    const sv = vertical(o.y, sub);
    const placed = { x: prev.x + prev.w + SUB_GAP, y: sv.y, w: sub.w, h: sub.h + sv.extra };
    out.push(placed);
    prev = placed;
  }
  return out;
}
