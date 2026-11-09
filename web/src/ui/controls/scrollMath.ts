// The model of the vertical scroll bar (port of `CWndScrollBar`, WndControl.cpp:1455-1800; Plan/42 §8.9) as pure
// functions: the numbers are goldens of the acceptance criteria, the control only wires them to the pointer.
//
// All values are in LINES (`min`, `max`, `page`, `pos`); the bar is `w` px wide (15) with an arrow button of `w x w`
// at both ends; the thumb lives on the track between them. The thumb is drawn only when `range > 0` and
// `range - page > 0`.

/** `CWndScrollBar::Create`: `rc.left = rc.right - 15`. */
export const SCROLLBAR_WIDTH = 15;
/** The shortest thumb (`nPadHeight < 6` -> 6 and the track shrinks by the difference). */
export const MIN_THUMB = 6;

/** A rectangle in bar-local px as `[top, height]` of a full-width part. */
export interface Thumb {
  /** Top of the thumb from the bar's top edge (arrow height + `pos * track / range`). */
  top: number;
  height: number;
  /** The track length the thumb positions were computed against (shrunk when the thumb is at its minimum). */
  track: number;
}

/** `range = max - min` (`GetScrollRange`). */
export function scrollRange(min: number, max: number): number {
  return max - min;
}

/** Whether the thumb is shown (`GetScrollRange() && GetScrollRange() - page > 0`). */
export function thumbVisible(min: number, max: number, page: number): boolean {
  const range = max - min;
  return range !== 0 && range - page > 0;
}

/**
 * The thumb of a bar `h` px high (`CWndScrollBar::OnDraw`): `nHeight = h - 2 * arrow`, `pad = page * nHeight / range`
 * (integer), a pad under 6 px becomes 6 and shortens `nHeight` by the difference, `top = arrow + pos * nHeight / range`.
 * `null` when the thumb is not shown or the track has no room.
 */
export function thumbGeometry(
  h: number,
  min: number,
  max: number,
  page: number,
  pos: number,
  arrow = SCROLLBAR_WIDTH,
): Thumb | null {
  if (!thumbVisible(min, max, page)) return null;
  const range = max - min;
  let track = h - 2 * arrow;
  let pad = Math.trunc((page * track) / range);
  if (pad < MIN_THUMB) {
    track -= MIN_THUMB - pad;
    pad = MIN_THUMB;
  }
  if (track <= 0) return null;
  return { top: arrow + Math.trunc((pos * track) / range), height: pad, track };
}

/**
 * The scroll position of a dragged thumb (`OnMouseMove` with `m_bPushPad`): `y` is the pointer's y in the bar,
 * `grab` the offset between the pointer and the thumb top at the press. Clamped to `[min, max - page]`.
 */
export function posFromDrag(
  y: number,
  grab: number,
  h: number,
  min: number,
  max: number,
  page: number,
  arrow = SCROLLBAR_WIDTH,
): number {
  const range = max - min;
  if (range === 0) return min;
  let track = h - 2 * arrow;
  const pad = Math.trunc((page * track) / range);
  if (pad < MIN_THUMB) track -= MIN_THUMB - pad;
  if (track <= 0) return min;
  let pos = Math.trunc(((y - (arrow + grab)) * range) / track);
  if (pos < min) pos = min;
  if (pos > max - page) pos = max - page;
  return pos;
}

/** A click on the track above (`-1`) or below (`1`) the thumb pages the position (`pos -= page` / `pos += page`). */
export function pagePos(dir: -1 | 1, pos: number, min: number, max: number, page: number): number {
  if (dir < 0) return Math.max(min, pos - page);
  return Math.min(max - page, pos + page);
}

/** The arrow commands `1000` (`--pos`, floor `min`) and `1001` (`++pos`, undone when it would pass `max - page`). */
export function stepPos(dir: -1 | 1, pos: number, min: number, max: number, page: number): number {
  if (dir < 0) return Math.max(min, pos - 1);
  const next = pos + 1;
  return next > max - page ? pos : next;
}

/** One wheel notch of Text, ListBox and Tree (`OnMouseWheel`): `notches < 0` scrolls down by a line, `>= 0` up. */
export function wheelPos(
  notches: number,
  pos: number,
  min: number,
  max: number,
  page: number,
): number {
  if (notches < 0) return max - page > pos ? pos + 1 : max - page;
  return min < pos ? pos - 1 : min;
}

/** The wheel does nothing while the page covers the whole range (`GetScrollPage() >= GetMaxScrollPos()`). */
export function wheelActive(max: number, page: number): boolean {
  return page < max;
}
