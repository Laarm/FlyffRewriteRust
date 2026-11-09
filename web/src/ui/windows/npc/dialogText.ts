// The parser of the dialog text (`CWndDialog::ParsingString`, `src:_Interface/WndDialog.cpp:225-310`, Plan/45 §11.3; WP-CFEAT-18):
// a page is markup text in which `[Display][Key]` marks a clickable word. The display word is drawn bold; the second bracket
// (never drawn) is the key the click sends, an EMPTY second bracket means key = display word (`[Just because][]`). A word that
// wraps onto the next line stays one link (the layout walks the marks, `dialogLayout.ts`). The marks per displayed character:
// 0 plain, 1 first character of a link word, 2 a middle one, 3 the last one, 4 a link word of one character.

import { DEFAULT_TEXT_COLOR, ESSTY, parseMarkup, type RichRun } from '../../text/markup';
import type { DialogPage } from './ports';

/** What the parser tells about malformed text (the original showed a message box; the port logs it). */
export interface DialogTextDiagnostics {
  /** Text between `]` and `[` (outside any bracket) after the display word was closed. */
  stray?(word: string, text: string): void;
}

interface Cell {
  readonly ch: string;
  readonly color: number;
  readonly bold: boolean;
  readonly underline: boolean;
  readonly strike: boolean;
}

/**
 * Parses one page. `markup` is the text after the `Say` prefixes (`"  " + text`, the quest title line) and the conversion of
 * the literal `\n`; `#b`, `#cAARRGGBB`, `#u` ... take effect first, then the brackets are read.
 */
export function parseDialogPage(
  markup: string,
  questId: number,
  diag: DialogTextDiagnostics = {},
): DialogPage {
  const cells: Cell[] = [];
  for (const run of parseMarkup(markup, DEFAULT_TEXT_COLOR)) {
    for (const ch of run.text) {
      cells.push({
        ch,
        color: run.color,
        bold: run.bold,
        underline: run.underline,
        strike: run.strike,
      });
    }
  }
  const display: Cell[] = [];
  const marks: number[] = [];
  const links = new Map<string, string>();
  let key = false;
  let brace = false;
  let nTemp = 0;
  let word = '';
  let original = '';
  for (const c of cells) {
    if (c.ch === '[') {
      brace = true;
      if (!key) {
        original = '';
        word = '';
        nTemp = 1;
      }
    } else if (c.ch === ']') {
      if (brace) {
        brace = false;
        if (!key) {
          key = true;
          // the last displayed character closes the word: 3, or 4 when the word is one character
          const n = Array.from(word).length;
          const last = marks.length - 1;
          if (last >= 0) marks[last] = n === 1 ? 4 : 3;
        } else {
          key = false;
          if (original === '') original = word;
          links.set(word, original);
        }
      }
    } else if (key) {
      if (brace) {
        original += c.ch;
      } else {
        // text between `]` and `[`: a data error of the script. The original swallowed it into the key; the port draws it
        // as plain text and reports it (Plan/45 §11.3)
        diag.stray?.(word, c.ch);
        display.push(c);
        marks.push(0);
      }
    } else {
      let mark = 0;
      let cell = c;
      if (brace) {
        mark = nTemp;
        if (nTemp === 1) nTemp += 1;
        cell = { ...c, bold: true, underline: false, strike: false };
      }
      marks.push(mark);
      display.push(cell);
      word += c.ch;
    }
  }
  return { runs: runsOf(display), marks, links, questId };
}

function runsOf(cells: readonly Cell[]): RichRun[] {
  const runs: RichRun[] = [];
  for (const c of cells) {
    const last = runs[runs.length - 1];
    if (
      last?.color === c.color &&
      last.bold === c.bold &&
      last.underline === c.underline &&
      last.strike === c.strike
    ) {
      last.text += c.ch;
    } else {
      runs.push({
        text: c.ch,
        color: c.color,
        bold: c.bold,
        underline: c.underline,
        strike: c.strike,
      });
    }
  }
  return runs;
}

/**
 * The text of a `say` operation before it is parsed (`CWndDialog::Say`): a page of a quest starts with the quest title in
 * bold blue-ish (`#b#cff5050f0<title>#nb#nc\n  `); every page text starts with two spaces; each literal backslash-n of the
 * script becomes a line break followed by two spaces.
 */
export function sayMarkup(text: string, questTitle: string | null): string {
  const body = text.replace(/\\n/g, '\n  ');
  if (questTitle !== null) return `#b#cff5050f0${questTitle}#nb#nc\n  ${body}`;
  return `  ${body}`;
}

/** Marks that a character is part of a link (for tests and the hover code). */
export const LINK_MARK = { NONE: 0, FIRST: 1, MIDDLE: 2, LAST: 3, SINGLE: 4 } as const;

export { ESSTY };
