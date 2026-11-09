// The rich-text markup of `CEditString::ParsingString` (`_Common/EditString.cpp:307-450`; Plan/42 §8.2, ref/22 §9).
//
//   #cAARRGGBB  set the colour (8 hex digits; FIX PL-124: upper case A-F is accepted, the original mis-parsed it)
//   #nc         back to the default colour of the call          #b / #nb   bold on / off  (ESSTY_BOLD 2)
//   #u / #nu    underline on / off (ESSTY_UNDERLINE 1)          #s / #ns   strike-through on / off (ESSTY_STRIKETHROUGH 4)
//   #lNNNN / #nl  code page switch / restore: consumed, no effect (DROP: UTF-8)
//   #<other>    a literal `#` and the character               \n (two characters)   a line break
//
// The module is pure: `parseMarkup(text, defaultColor)` returns `RichRun[]`, the unit the line breaker
// (`layout.ts`), the renderer (`RichText.tsx`) and the tooltips use.

/** `ESSTY_*`: the style bits of a character (`EditString.h`). */
export const ESSTY = { UNDERLINE: 1, BOLD: 2, STRIKETHROUGH: 4 } as const;

/** One run of characters with the same colour and style. Colours are `0xAARRGGBB`. */
export interface RichRun {
  text: string;
  color: number;
  bold: boolean;
  underline: boolean;
  strike: boolean;
}

/** The colour the original gives text when the caller passes none (`0xff000000`, black). */
export const DEFAULT_TEXT_COLOR = 0xff000000;

/** `D3DCOLOR_ARGB`. */
export function argb(a: number, r: number, g: number, b: number): number {
  return (((a & 255) << 24) | ((r & 255) << 16) | ((g & 255) << 8) | (b & 255)) >>> 0;
}

/** The CSS colour of an `0xAARRGGBB` value (`#rrggbb` when opaque, `rgba()` otherwise). */
export function cssColor(color: number): string {
  const c = color >>> 0;
  const a = c >>> 24;
  const r = (c >>> 16) & 255;
  const g = (c >>> 8) & 255;
  const b = c & 255;
  if (a === 255) return `rgb(${String(r)},${String(g)},${String(b)})`;
  return `rgba(${String(r)},${String(g)},${String(b)},${(a / 255).toFixed(3)})`;
}

const HEX = /^[0-9a-fA-F]{8}$/;

/**
 * Parses the markup into runs.
 *
 * @param macro `true` (default, `PS_USE_MACRO`): the codes take effect. `false` (`PS_NOT_MACRO`): the codes are
 *   CONSUMED and apply nothing (the original swallows `#cAARRGGBB`, `#b`, ... and `#x` without printing them; the
 *   `\n` conversion still happens). Plan/42 §8.2 describes this case as "literal"; the source (EditString.cpp:373-415)
 *   drops the codes, and the source wins.
 */
export function parseMarkup(
  text: string,
  defaultColor: number = DEFAULT_TEXT_COLOR,
  macro = true,
): RichRun[] {
  const runs: RichRun[] = [];
  let color = defaultColor >>> 0;
  let bold = false;
  let underline = false;
  let strike = false;
  let buf = '';
  const flush = (): void => {
    if (buf === '') return;
    const last = runs[runs.length - 1];
    if (
      last?.color === color &&
      last.bold === bold &&
      last.underline === underline &&
      last.strike === strike
    ) {
      last.text += buf;
    } else {
      runs.push({ text: buf, color, bold, underline, strike });
    }
    buf = '';
  };
  const emit = (s: string): void => {
    buf += s;
  };
  const n = text.length;
  for (let i = 0; i < n; i += 1) {
    const ch = text.charAt(i);
    if (ch === '#') {
      i += 1;
      if (i >= n) break; // a trailing `#` is dropped
      const code = text.charAt(i);
      if (code === 'c') {
        i += 1;
        if (i >= n) break;
        const digits = text.slice(i, i + 8);
        if (macro && HEX.test(digits)) {
          flush();
          color = Number.parseInt(digits, 16) >>> 0;
        }
        i += 7; // the eight characters are consumed whatever they were
      } else if (code === 'u') {
        if (macro) {
          flush();
          underline = true;
        }
      } else if (code === 'b') {
        if (macro) {
          flush();
          bold = true;
        }
      } else if (code === 's') {
        if (macro) {
          flush();
          strike = true;
        }
      } else if (code === 'l') {
        i += 1;
        if (i >= n) break;
        i += 3; // `#l` + four digits: the code page is dropped
      } else if (code === 'n') {
        i += 1;
        if (i >= n) break;
        if (macro) {
          const which = text.charAt(i);
          if (which === 'c') {
            flush();
            color = defaultColor >>> 0;
          } else if (which === 'b') {
            flush();
            bold = false;
          } else if (which === 'u') {
            flush();
            underline = false;
          } else if (which === 's') {
            flush();
            strike = false;
          }
          // `#nl` restores the code page: nothing to do; any other `#nX` is consumed silently
        }
      } else if (macro) {
        emit('#');
        emit(code);
      }
    } else if (ch === '\\' && text.charAt(i + 1) === 'n') {
      emit('\n');
      i += 1;
    } else {
      emit(ch);
    }
  }
  flush();
  return runs;
}

/** The plain text of runs (markup removed). */
export function runsText(runs: readonly RichRun[]): string {
  let s = '';
  for (const r of runs) s += r.text;
  return s;
}

/** One run of a single colour and no style (the plain-text path: `setString` without markup). */
export function plainRuns(text: string, color: number = DEFAULT_TEXT_COLOR): RichRun[] {
  return text === ''
    ? []
    : [{ text, color: color >>> 0, bold: false, underline: false, strike: false }];
}

/** The runs without the characters `[from, to)` (code point offsets of the text, `\r` already removed). */
export function deleteRange(runs: readonly RichRun[], from: number, to: number): RichRun[] {
  const out: RichRun[] = [];
  let pos = 0;
  for (const run of runs) {
    const chars = Array.from(run.text);
    const keep = chars.filter((_c, i) => pos + i < from || pos + i >= to).join('');
    pos += chars.length;
    if (keep === '') continue;
    const last = out[out.length - 1];
    if (
      last?.color === run.color &&
      last.bold === run.bold &&
      last.underline === run.underline &&
      last.strike === run.strike
    ) {
      last.text += keep;
    } else {
      out.push({ ...run, text: keep });
    }
  }
  return out;
}

/** The runs with every `\r` removed (`layoutLines` skips them; offsets of lines then match the stored text). */
export function stripCr(runs: readonly RichRun[]): RichRun[] {
  return runs
    .map((r) => (r.text.includes('\r') ? { ...r, text: r.text.replaceAll('\r', '') } : r))
    .filter((r) => r.text !== '');
}
