// The append rule of both chat text areas (`CWndChat::PutString` / `CWndChatLog::PutString`, WndCommunication.cpp:969 and
// 1458): add the line (markup parsed) and a newline, drop the first 500 lines when the area reaches 1000, and keep the
// view at the bottom only when it was at the bottom.

import type { TextCtrl } from '../controls/TextCtrl';

/** `GetLineCount() >= 1000`: ... */
export const TEXT_TRIM_AT = 1000;
/** ... `DeleteLine(0, 500)`. */
export const TEXT_TRIM_DROP = 500;

/** The view shows the last line (`pos == max - page`; a text shorter than the page counts as at the bottom). */
export function atBottom(text: TextCtrl): boolean {
  const bar = text.scrollBar;
  if (!bar) return true;
  return bar.getScrollPos() >= Math.max(0, bar.getMaxScrollPos() - bar.getScrollPage());
}

/**
 * The text of a chat line is always drawn opaque: the legacy `CEditString::SetAlpha` replaces the alpha byte of every
 * character (the window fade), so a colour written as `0xff0000` (alpha 0, as the C++ does in places) still shows.
 */
export const opaque = (color: number): number => (color | 0xff000000) >>> 0;

export function appendChatLine(text: TextCtrl, line: string, color: number): void {
  const follow = atBottom(text);
  text.addString(`${line}\n`, opaque(color));
  if (text.getLineCount() >= TEXT_TRIM_AT) text.deleteLines(0, TEXT_TRIM_DROP);
  if (follow) text.scrollToEnd();
}
