// The geometry of the chat window controls (`CWndChat::OnSize`, WndCommunication.cpp:1017-1120): every position is a
// function of the client size (`GetClientRect()` without the parent flag: `(0, 0, cw, ch)`), so the rules are
// testable without a window.

import type { Point, Rect } from '../layout/geometry';

/** `AdjustMinRect(&rectWnd, 16 * 29, 16 * 3)`. */
export const CHAT_MIN_W = 16 * 29;
export const CHAT_MIN_H = 16 * 3;
/** `AdjustMinRect(&rectWnd, 16 * 17, 16 * 3)` of the system log window. */
export const LOG_MIN_W = 16 * 17;
export const LOG_MIN_H = 16 * 3;

/** The x of the five channel radios. */
export const RADIO_X = [0, 50, 100, 150, 200] as const;

export interface ChatGeometry {
  /** The text area rect: `(0, 0, right, bottom - 39)`. */
  text: Rect;
  /** Top and bottom of the (hidden) head edit and of the macro icon. */
  headY: { t: number; b: number };
  macroY: { t: number; b: number };
  /** Top, bottom and right of the input edit. */
  edit: { t: number; b: number; r: number };
  /** Top-left of the radios (index = tab) and of the other controls. */
  radios: Point[];
  lock: Point;
  filter: Point;
  system: Point;
  moveLock: Point;
  help: Point;
  close: Point;
  imeStatic: Point;
}

/** Positions of everything in the client rect `c = (0, 0, cw, ch)`. */
export function chatGeometry(c: Rect): ChatGeometry {
  const rowY = c.b - 38;
  const second = rowY + 1;
  return {
    text: { l: 0, t: 0, r: c.r, b: c.b - 39 },
    headY: { t: c.b - 18, b: c.b },
    macroY: { t: c.b - 19, b: c.b },
    edit: { t: c.b - 18, b: c.b, r: c.r - 20 },
    radios: RADIO_X.map((x) => ({ x, y: rowY })),
    lock: { x: c.r - 200, y: rowY },
    filter: { x: c.r - 150, y: rowY },
    system: { x: c.r - 100, y: rowY },
    moveLock: { x: c.r - 49, y: second },
    help: { x: c.r - 32, y: second },
    close: { x: c.r - 15, y: second },
    imeStatic: { x: c.r - 15 - 5, y: second + 20 },
  };
}

/** The x of the system log window next to the chat window (`rect.right + w < layout.right` ? right of it : left of it). */
export function logWindowX(chat: Rect, logWidth: number, layoutRight: number): number {
  return chat.r + logWidth < layoutRight ? chat.r : chat.l - logWidth;
}
