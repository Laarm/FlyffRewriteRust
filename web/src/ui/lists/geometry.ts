// Pure geometry of the specialised grids and lists (WP-UI-23): which slot or row sits under a pointer, which rows are visible
// at a scroll position, how a thousands-separated price reads and which colour a price has. Kept out of the controls so the
// numbers are goldens of the tests (the original computed them inline in each `OnDraw` / `HitTest`).

import type { Point, Rect } from '../layout/geometry';

/** A fixed layout of slots (the vendor, trade and merit grids): where the icon of slot `i` is and what a press on it hits. */
export interface SlotLayout {
  /** The number of slots (`MAX_VENDOR_REVISION`, `MAX_TRADE`, the container size). */
  readonly count: number;
  /** The slots of a row (the scroll unit is a row of slots). */
  readonly columns: number;
  /** The icon origin of slot `slot` in client coordinates when nothing is scrolled. */
  origin(slot: number): Point;
  /** The hit rect of slot `slot` in client coordinates when nothing is scrolled. */
  hit(slot: number): Rect;
  /** The vertical distance of two rows (the scroll step in px). */
  readonly rowPitch: number;
}

/** The slot under a client point, `-1` for none. `scroll` is the first visible row. */
export function slotAtPoint(layout: SlotLayout, pt: Point, scroll: number): number {
  const shift = scroll * layout.rowPitch;
  for (let i = scroll * layout.columns; i < layout.count; i += 1) {
    const r = layout.hit(i);
    if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t - shift && pt.y < r.b - shift) return i;
  }
  return -1;
}

/** The rows of a list that are visible: `[first, last)` for a scroll position, a page and a row count, plus `extra` partial rows. */
export function visibleRows(
  scroll: number,
  page: number,
  count: number,
  extra = 1,
): readonly [number, number] {
  const first = Math.max(0, Math.min(scroll, count));
  const last = Math.min(count, first + Math.max(0, page) + extra);
  return [first, last];
}

/** The row under a client y of a list whose rows start at `top` and are `pitch` px apart, `-1` for none. */
export function rowAtY(
  y: number,
  top: number,
  pitch: number,
  scroll: number,
  count: number,
): number {
  if (pitch <= 0 || y < top) return -1;
  const row = scroll + Math.floor((y - top) / pitch);
  return row >= 0 && row < count ? row : -1;
}

/** `GetNumberFormatEx`: the digits grouped by thousands with commas (`1234567` -> `1,234,567`). */
export function groupThousands(n: number): string {
  const digits = String(Math.trunc(Math.abs(n)));
  const out = digits.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return n < 0 ? `-${out}` : out;
}

/**
 * The colour of a vendor price by magnitude (`CWndVendorCtrl::OnDrawItemInfo`): black up to 99,999, green from 100,000
 * (10 man), blue from 1,000,000, orange from 10,000,000, red from 100,000,000, purple from 1,000,000,000.
 */
export function priceColor(price: number): number {
  if (price >= 1_000_000_000) return 0xff990099;
  if (price >= 100_000_000) return 0xffcc0303;
  if (price >= 10_000_000) return 0xffff6600;
  if (price >= 1_000_000) return 0xff3333ff;
  if (price >= 100_000) return 0xff009900;
  return 0xff000000;
}

/**
 * Cuts a text to `maxWidth` px with an ellipsis (`MAX_VENDOR_ITEM_TEXT_EXTENT`, 108): the longest prefix that fits, then
 * `...`; a text that fits is returned as it is. `measure` is the text width of the control's font.
 */
export function ellipsize(text: string, maxWidth: number, measure: (s: string) => number): string {
  if (measure(text) <= maxWidth) return text;
  let kept = '';
  for (const ch of text) {
    if (measure(kept + ch) > maxWidth) break;
    kept += ch;
  }
  return `${kept}...`;
}

/** `CTimeSpan` split the way the premium item list prints it. */
export function splitDuration(seconds: number): {
  readonly days: number;
  readonly hours: number;
  readonly minutes: number;
  readonly seconds: number;
} {
  const total = Math.max(0, Math.trunc(seconds));
  return {
    days: Math.floor(total / 86400),
    hours: Math.floor((total % 86400) / 3600),
    minutes: Math.floor((total % 3600) / 60),
    seconds: total % 60,
  };
}

/**
 * `string.Format(prj.GetText(TID_TOOLTIP_DATE), days, hours, minutes, seconds)`: fills the `%d` / `%02d` conversions of the
 * text template in order with days, hours, minutes and seconds (a conversion past the fourth stays as it is).
 */
export function formatDuration(template: string, seconds: number): string {
  const d = splitDuration(seconds);
  const values = [d.days, d.hours, d.minutes, d.seconds];
  let i = 0;
  return template.replace(/%0?(\d)?d/g, (m, width: string | undefined) => {
    const v = values[i];
    if (v === undefined) return m;
    i += 1;
    const w = width === undefined ? 0 : Number(width);
    return String(v).padStart(w, '0');
  });
}
