// The numbers of the status window as pure functions (`CWndStatus::OnDraw`, WndField.cpp:10905-11120, 45 §7.2; WP-UI-21): the
// display modes, the number formats and the positions of every text. The positions are the original's `TextOut` arguments -
// including its approximations of the text width (`nChar * 5.8`, `nChar / 2 * width("8")`).

import type { Rect } from '../../layout/geometry';
import { FONT, lineHeight, textWidth } from '../../text/fonts';
import type { TextItem } from '../../controls/TextLayer';
import type { StatusView } from './ports';

/** `D3DCOLOR_ARGB(255, 255, 255, 255)`: every number of the window is white (in the outlined title font). */
export const STATUS_TEXT_COLOR = 0xffffffff;

/** `m_nDisplay`: 0 nothing, 1 percent and value, 2 value / maximum. Initial 2; a click goes 2 -> 0 -> 1 -> 2. */
export type StatusDisplay = 0 | 1 | 2;
export const STATUS_DISPLAY_INITIAL: StatusDisplay = 2;

export function nextStatusDisplay(d: StatusDisplay): StatusDisplay {
  return d === 2 ? 0 : d === 0 ? 1 : 2;
}

/** `Separator(n, '.')`: thousands separated by dots (`1234567` -> `1.234.567`). */
export function separatorDots(n: number): string {
  const sign = n < 0 ? '-' : '';
  const digits = String(Math.abs(Math.trunc(n)));
  let out = '';
  for (let i = 0; i < digits.length; i += 1) {
    if (i > 0 && (digits.length - i) % 3 === 0) out += '.';
    out += digits[i] ?? '';
  }
  return sign + out;
}

/**
 * The value text of the HP / MP / FP line: `%.1fk` from 10 000, `%.1fm` from a million, `%.1fb` from a billion, else the dotted
 * number. (`float(n) / 1000` with `%.1f`: JavaScript's `toFixed` rounds ties up like the MSVC runtime of the original.)
 */
export function compactNumber(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}b`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}m`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return separatorDots(n);
}

/** `GetHitPointPercent()`: `n * 100 / max` in integer arithmetic (0 for an empty maximum). */
export function percentOf(value: number, max: number): number {
  return max > 0 ? Math.trunc((value * 100) / max) : 0;
}

/** The EXP text: `"99.99%"` from 99.99, else `%.2f%%` of `x100 / 100` (the legacy code avoids a rounded "100.00%"). */
export function expPercentText(x100: number): string {
  const f = x100 / 100;
  if (f >= 99.99) return '99.99%';
  return `${f.toFixed(2)}%`;
}

/** The rects of the five gauges, in client coordinates. */
export interface StatusRects {
  readonly hp: Rect;
  readonly mp: Rect;
  readonly fp: Rect;
  readonly exp: Rect;
  readonly rest: Rect;
}

/** `nTopGap`: 2 for the French and German texts, else the line height of the text font beyond 14 px. */
export function statusTopGap(frGer = false): number {
  if (frGer) return 2;
  const h = lineHeight(FONT.text);
  return h > 14 ? h - 14 : 0;
}

/** Every text of the window for a view and a display mode. */
export function statusTexts(o: {
  readonly view: StatusView;
  readonly display: StatusDisplay;
  readonly rects: StatusRects;
  readonly frGer?: boolean;
}): TextItem[] {
  const { view, display, rects } = o;
  if (display === 0) return [];
  const items: TextItem[] = [];
  const gap = statusTopGap(o.frGer === true);
  const put = (x: number, y: number, text: string): void => {
    items.push({ x, y, text, color: STATUS_TEXT_COLOR, font: 'title' });
  };
  const digit = textWidth(FONT.text, '8');
  const lines = [
    { r: rects.hp, v: view.hp, m: view.hpMax },
    { r: rects.mp, v: view.mp, m: view.mpMax },
    { r: rects.fp, v: view.fp, m: view.fpMax },
  ];
  if (view.improved) {
    if (display === 1) {
      for (const l of lines) put(l.r.r - 90, l.r.t - gap, `${String(percentOf(l.v, l.m))}%`);
      for (const l of lines) {
        const t = compactNumber(l.v);
        put(l.r.r - 17 - Math.trunc(t.length * 5.8), l.r.t - gap, t);
      }
    } else {
      for (const l of lines) {
        const t = compactNumber(l.v);
        put(l.r.r - 82 - Math.trunc((t.length / 2) * digit), l.r.t - gap, t);
      }
      for (const l of lines) {
        const t = compactNumber(l.m);
        put(l.r.r - 30 - Math.trunc((t.length / 2) * digit), l.r.t - gap, t);
      }
      const width = rects.hp.r - rects.hp.l;
      const slashX = Math.trunc(width / 2 + digit / 2);
      for (const l of lines) put(l.r.r - slashX, l.r.t - gap, '/');
    }
  }
  // the EXP percent hangs off the HP rect's right edge (`x = lpHP->rect.right - 7`)
  const exp = expPercentText(view.expPercentX100);
  const x = rects.hp.r - 7;
  put(x - Math.trunc(exp.length * 5.8), rects.exp.t, exp);
  if (view.restText) {
    const rest = `${String(view.restPoint)}%`;
    put(x - Math.trunc(rest.length * 5.8), rects.rest.t, rest);
  }
  return items;
}
