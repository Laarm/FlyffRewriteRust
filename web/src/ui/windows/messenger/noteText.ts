// The text of the notes window (`CWndMessengerNote::OnInitialUpdate`, `src:_Interface/WndMessenger.cpp:1380-1410`): each note is `<from> - YYYY / MM / DD` in brown, the text
// indented by two blanks, a blank line. Pure functions, so the date goldens run without a window.

import type { NoteEntry } from './ports';

/** The colour of a note header (`0xff804000`). */
export const NOTE_HEADER_COLOR = 0xff804000;

const two = (n: number): string => String(n).padStart(2, '0');

/** `"%s - %d / %02d / %02d"` of the integer `YYYYMMDD` (`date / 10000`, `( date % 10000 ) / 100`, `date % 100`). */
export function noteHeader(from: string, date: number): string {
  const d = Math.trunc(date);
  const year = Math.trunc(d / 10000);
  const month = Math.trunc((d % 10000) / 100);
  const day = (d % 10000) % 100;
  return `${from} - ${String(year)} / ${two(month)} / ${two(day)}`;
}

/** The pieces `AddString` receives for one note: header (brown), the indent, the text, the closing blank lines. */
export function notePieces(n: NoteEntry): { text: string; color: number | null; macro: boolean }[] {
  return [
    { text: noteHeader(n.from, n.date), color: NOTE_HEADER_COLOR, macro: false },
    { text: '\n  ', color: null, macro: true },
    { text: n.text, color: null, macro: false },
    { text: '\n\n', color: null, macro: true },
  ];
}

/** The 255-byte rule of a note (`strlen( lpSendMessage ) <= 255`): the UTF-8 length. */
export const MAX_NOTE_BYTES = 255;
export function noteFits(text: string): boolean {
  return new TextEncoder().encode(text).length <= MAX_NOTE_BYTES;
}
