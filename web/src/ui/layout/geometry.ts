// Rectangle helpers and the frame/control geometry rules (Plan/42 §3.4, ref/22 §4.1; WndBase.cpp:1834-1858).
// All values are integer UI virtual pixels. Rectangles are half-open like `CRect`: a point is inside when
// `l <= x < r && t <= y < b`.

import {
  WBS_CAPTION,
  WBS_NOFRAME,
  WBS_VSCROLL,
  WTYPE_COMBOBOX,
  WTYPE_EDITCTRL,
  WTYPE_LISTBOX,
  WTYPE_STATIC,
  WTYPE_TEXT,
  WTYPE_TREECTRL,
} from './styles';

export interface Rect {
  l: number;
  t: number;
  r: number;
  b: number;
}
export interface Point {
  x: number;
  y: number;
}
export interface Size {
  w: number;
  h: number;
}

export const rect = (l: number, t: number, r: number, b: number): Rect => ({ l, t, r, b });
export const rectXYWH = (x: number, y: number, w: number, h: number): Rect => ({
  l: x,
  t: y,
  r: x + w,
  b: y + h,
});
/** A rect from the `[left, top, right, bottom]` tuple of a layout row (copied). */
export const rectFromTuple = (t: readonly [number, number, number, number]): Rect => ({
  l: t[0],
  t: t[1],
  r: t[2],
  b: t[3],
});
export const rectWidth = (r: Rect): number => r.r - r.l;
export const rectHeight = (r: Rect): number => r.b - r.t;
export const rectTopLeft = (r: Rect): Point => ({ x: r.l, y: r.t });
export const rectEq = (a: Rect, b: Rect): boolean =>
  a.l === b.l && a.t === b.t && a.r === b.r && a.b === b.b;
export const rectCopy = (r: Rect): Rect => ({ l: r.l, t: r.t, r: r.r, b: r.b });
/** `CRect::OffsetRect`. */
export const rectOffset = (r: Rect, dx: number, dy: number): Rect => ({
  l: r.l + dx,
  t: r.t + dy,
  r: r.r + dx,
  b: r.b + dy,
});
/** `CRect::DeflateRect(l, t, r, b)`: each edge moves inwards. */
export const rectDeflate = (r: Rect, l: number, t: number, rr: number, b: number): Rect => ({
  l: r.l + l,
  t: r.t + t,
  r: r.r - rr,
  b: r.b - b,
});
/** `CRect::PtInRect` (half-open). */
export const rectContains = (r: Rect, p: Point): boolean =>
  p.x >= r.l && p.x < r.r && p.y >= r.t && p.y < r.b;
export const rectIsEmpty = (r: Rect): boolean => r.r <= r.l || r.b <= r.t;
/** The intersection; an empty rect (`r == l`, `b == t`) when they do not overlap. */
export function rectIntersect(a: Rect, b: Rect): Rect {
  const l = Math.max(a.l, b.l);
  const t = Math.max(a.t, b.t);
  const r = Math.min(a.r, b.r);
  const bottom = Math.min(a.b, b.b);
  return r > l && bottom > t ? { l, t, r, b: bottom } : { l, t, r: l, b: t };
}

/** Rounds `n` up to a multiple of 16 (tiled frames, `CWndNeuz::OnSize`, WndNeuz.cpp:493-512). */
export const roundUp16 = (n: number): number => Math.ceil(n / 16) * 16;

/** The size of a tiled frame: width and height rounded up to a multiple of 16. */
export const tiledSize = (w: number, h: number): Size => ({ w: roundUp16(w), h: roundUp16(h) });

/** Height of the caption band that is added to the client top (`WBS_CAPTION`). */
export const CAPTION_HEIGHT = 18;
/** `DeflateRect(4, 4, 8, 10)` of a framed window (left, top, right, bottom). */
export const CLIENT_DEFLATE = { l: 4, t: 4, r: 8, b: 10 } as const;
/** `m_rectLayout = m_rectClient.Deflate(8, 8)`. */
export const LAYOUT_DEFLATE = 8;

/** The three rects of a window (parent-relative, ref/22 §4.1). */
export interface FrameRects {
  window: Rect;
  client: Rect;
  layout: Rect;
}

/**
 * `CWndBase::SetWndRect` (WndBase.cpp:1834-1858): the client and layout rect of a window.
 * `root` windows and `WBS_NOFRAME` windows have client = layout = window.
 */
export function frameRects(window: Rect, style: number, root = false): FrameRects {
  let client = rectCopy(window);
  let layout = rectCopy(window);
  if (!root && (style & WBS_NOFRAME) === 0) {
    if ((style & WBS_CAPTION) !== 0) {
      client = { ...client, t: client.t + CAPTION_HEIGHT };
    }
    client = rectDeflate(
      client,
      CLIENT_DEFLATE.l,
      CLIENT_DEFLATE.t,
      CLIENT_DEFLATE.r,
      CLIENT_DEFLATE.b,
    );
    layout = rectDeflate(client, LAYOUT_DEFLATE, LAYOUT_DEFLATE, LAYOUT_DEFLATE, LAYOUT_DEFLATE);
  }
  return { window: rectCopy(window), client, layout };
}

/**
 * Offset of a frame's client rect from its window rect: `(0, 0)` for `WBS_NOFRAME`, else `(4, 4)` plus 18 with a
 * caption. Children of a frame are positioned relative to its CLIENT rect (docking children excepted).
 */
export function clientOffset(style: number, root = false): Point {
  if (root || (style & WBS_NOFRAME) !== 0) return { x: 0, y: 0 };
  return {
    x: CLIENT_DEFLATE.l,
    y: CLIENT_DEFLATE.t + ((style & WBS_CAPTION) !== 0 ? CAPTION_HEIGHT : 0),
  };
}

/**
 * A layout control rect `(l, t, r, b)` in frame-window coordinates: `(l+4, t+4+18·caption, r+4, b+4+18·caption)`
 * in captioned frames and `(l, t, r, b)` in `WBS_NOFRAME` frames (Plan/42 §3.4; the §21.3 conformance mapping).
 * Negative rects stay negative (never clamped).
 */
export function controlRectInFrame(ctrl: Rect, frameStyle: number): Rect {
  const o = clientOffset(frameStyle);
  return rectOffset(ctrl, o.x, o.y);
}

/**
 * The client rect of a control from its window rect (both in the parent's client coordinates): the
 * `SetWndRect` overrides of the control classes (WndControl.cpp:38-45, 1140, 1317, 2639, 2728; Plan/42 §3.4):
 * Text deflates by (6, 6) unless `WBS_NOFRAME` and loses 15 px on the right with `WBS_VSCROLL`; Edit and Combo
 * deflate (3, 3, 3, 0); Static has client = window; ListBox and TreeCtrl deflate (4, 4) and lose 15 px with `WBS_VSCROLL`; everything else
 * follows the frame rule (`WBS_NOFRAME` controls: client = window).
 */
export function controlClientRect(type: number, style: number, window: Rect): Rect {
  switch (type) {
    case WTYPE_TEXT: {
      let c = (style & WBS_NOFRAME) === 0 ? rectDeflate(window, 6, 6, 6, 6) : rectCopy(window);
      if ((style & WBS_VSCROLL) !== 0) c = { ...c, r: c.r - 15 };
      return c;
    }
    case WTYPE_STATIC:
      return rectCopy(window);
    case WTYPE_EDITCTRL:
    case WTYPE_COMBOBOX:
      return rectDeflate(window, 3, 3, 3, 0);
    case WTYPE_LISTBOX:
    case WTYPE_TREECTRL: {
      const c = rectDeflate(window, 4, 4, 4, 4);
      return (style & WBS_VSCROLL) !== 0 ? { ...c, r: c.r - 15 } : c;
    }
    default:
      return frameRects(window, style).client;
  }
}

/** `CWndBase::MoveRectCenter`: the window rect of size `(w, h)` centred in `area` (integer division). */
export function centerIn(area: Rect, w: number, h: number): Rect {
  const x = area.l + Math.trunc(rectWidth(area) / 2) - Math.trunc(w / 2);
  const y = area.t + Math.trunc(rectHeight(area) / 2) - Math.trunc(h / 2);
  return rectXYWH(x, y, w, h);
}
