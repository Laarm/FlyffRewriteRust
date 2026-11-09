// The line breaker of the rich text (port of `CEditString::Align`, `_Common/EditString.cpp:682-782`; Plan/42 §8.3).
//
// Lines are computed here, not by CSS wrapping, so that every line count, scroll range and tooltip box equals the
// original's whatever font is loaded:
//  1. walk the characters, accumulating per-character advances (no kerning); `\n` ends the line, `\r` is skipped;
//  2. a break opportunity is remembered AFTER any of ` +-*^/%=;(),':{}.` (the character itself stays on the line);
//  3. when a NON-space character makes the width exceed the box: with a break opportunity on this line the next line
//     starts after it (word alignment, ENG), otherwise the line breaks before the character. A space never breaks
//     (it may overhang).
// A character that does not fit even alone is placed on its own line (the original would loop forever there).

import { type FontSpec, charWidth } from './fonts';
import type { RichRun } from './markup';

/** The characters after which a line may break (`strchr(" +-*^/%=;(),':{}.", chr)`). */
export const BREAK_CHARS = " +-*^/%=;(),':{}.";

/** One laid-out line: runs without the `\n`, the width of the line in px. */
export interface RichLine {
  runs: RichRun[];
  /** The sum of the character advances of the line. */
  width: number;
  /** Offset of the line's first character in the text (code points, `\r` removed): `GetLineOffset`. */
  offset: number;
  /** Characters of the text this line owns up to the next line, its terminating `\n` included. */
  length: number;
}

/** The advance of one character in px. */
export type CharMeasure = (ch: string) => number;

/** The plain text of a line. */
export function lineText(line: RichLine): string {
  let s = '';
  for (const r of line.runs) s += r.text;
  return s;
}

/**
 * Breaks `runs` into lines for a box of `width` px.
 *
 * @param wordAlign the ENG rule of step 3 (`m_bWordAlign`); `false` for edit controls (character wrap only)
 * @returns no lines for an empty text; a text ending in `\n` has no empty last line (`Align` stops at the end)
 */
export function layoutLines(
  runs: readonly RichRun[],
  width: number,
  measure: CharMeasure,
  wordAlign = true,
): RichLine[] {
  const chars: string[] = [];
  const runOf: number[] = [];
  runs.forEach((run, ri) => {
    for (const ch of run.text) {
      if (ch === '\r') continue;
      chars.push(ch);
      runOf.push(ri);
    }
  });
  const n = chars.length;
  const spans: { start: number; end: number; next: number }[] = [];
  let offset = 0;
  while (offset < n) {
    const start = offset;
    let w = 0;
    let nSpace = 0;
    let end: number;
    for (;;) {
      const ch = chars[offset] ?? '';
      if (BREAK_CHARS.includes(ch)) nSpace = offset + 1;
      if (ch === '\n') {
        end = offset;
        offset += 1;
        break;
      }
      w += measure(ch);
      if (ch !== ' ' && w > width) {
        if (wordAlign && nSpace > start) offset = nSpace;
        else if (offset === start) offset += 1; // nothing fits: one character per line
        end = offset;
        break;
      }
      offset += 1;
      if (offset >= n) {
        end = n;
        break;
      }
    }
    spans.push({ start, end, next: offset });
  }
  return spans.map(({ start, end, next }) => {
    const out: RichRun[] = [];
    let lw = 0;
    let curRun = -1;
    for (let i = start; i < end; i += 1) {
      const ri = runOf[i] ?? 0;
      const ch = chars[i] ?? '';
      lw += measure(ch);
      const src = runs[ri];
      if (!src) continue;
      if (ri === curRun) {
        const last = out[out.length - 1];
        if (last) last.text += ch;
      } else {
        out.push({ ...src, text: ch });
        curRun = ri;
      }
    }
    return { runs: out, width: lw, offset: start, length: next - start };
  });
}

/** `layoutLines` with the advances of a font slot (`charWidth`, cached). */
export function layoutRich(
  runs: readonly RichRun[],
  width: number,
  font: FontSpec,
  wordAlign = true,
): RichLine[] {
  return layoutLines(runs, width, (ch) => charWidth(font, ch), wordAlign);
}

/**
 * `GetTextExtent_EditString`: the widest line of the laid-out text (width) and `lines * cell` (height; outlined fonts
 * pass their cell with the outline already added).
 */
export function textExtent(lines: readonly RichLine[], cell: number): { w: number; h: number } {
  let w = 0;
  for (const l of lines) if (l.width > w) w = l.width;
  return { w, h: lines.length * cell };
}
