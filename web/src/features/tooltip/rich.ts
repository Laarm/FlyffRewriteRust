// `CEditString` as the tooltip composer uses it: a sequence of `AddString(text, color, style)` calls that become the rich runs the
// tooltip host lays out (`RichRun`, Plan/42 §8.2). Adjacent additions with the same colour and style share one run (the
// original stores a colour per character, so the merge changes nothing visible). Plus the `CTimeSpan` split the time lines use.

import { ESSTY, type RichRun } from '../../ui/text/markup';

export { ESSTY };

/** `ESSTY_*` style bits of an `AddString` call. */
export type StyleBits = number;

export class RichBuilder {
  readonly runs: RichRun[] = [];

  /** `CEditString::AddString(text, color, style)`. */
  add(text: string, color: number, style: StyleBits = 0): this {
    if (text === '') return this;
    const bold = (style & ESSTY.BOLD) !== 0;
    const underline = (style & ESSTY.UNDERLINE) !== 0;
    const strike = (style & ESSTY.STRIKETHROUGH) !== 0;
    const c = color >>> 0;
    const last = this.runs[this.runs.length - 1];
    if (
      last?.color === c &&
      last.bold === bold &&
      last.underline === underline &&
      last.strike === strike
    ) {
      last.text += text;
    } else {
      this.runs.push({ text, color: c, bold, underline, strike });
    }
    return this;
  }

  /** Appends runs built elsewhere (`PutDestParam`). */
  addRuns(runs: readonly RichRun[]): this {
    for (const r of runs) {
      this.add(
        r.text,
        r.color,
        (r.bold ? ESSTY.BOLD : 0) |
          (r.underline ? ESSTY.UNDERLINE : 0) |
          (r.strike ? ESSTY.STRIKETHROUGH : 0),
      );
    }
    return this;
  }

  /** The plain text so far (tests, the default line count). */
  get text(): string {
    return this.runs.map((r) => r.text).join('');
  }
}

/** `CTimeSpan(seconds)`: days, hours (0-23), minutes (0-59), seconds (0-59) and the totals. */
export interface TimeSpan {
  readonly days: number;
  readonly hours: number;
  readonly minutes: number;
  readonly seconds: number;
  readonly totalSeconds: number;
  readonly totalMinutes: number;
  readonly totalHours: number;
}

export function timeSpan(totalSeconds: number): TimeSpan {
  const t = Math.trunc(totalSeconds);
  const abs = Math.abs(t);
  const sign = t < 0 ? -1 : 1;
  return {
    days: sign * Math.floor(abs / 86400),
    hours: sign * (Math.floor(abs / 3600) % 24),
    minutes: sign * (Math.floor(abs / 60) % 60),
    seconds: sign * (abs % 60),
    totalSeconds: t,
    totalMinutes: Math.trunc(t / 60),
    totalHours: Math.trunc(t / 3600),
  };
}
