// Number formatting helpers of the feature windows (Plan/45 §2.4; legacy `GetNumberFormatEx`, `Separator` of
// `src:_Common/xUtil.cpp:395-420`, and the `__IMPROVED_STATUS` compact form).

/**
 * Groups the decimal digits of `text` by three from the right with `sep` (legacy `GetNumberFormatEx` / `Separator`).
 * KEEP-QUIRK: every third CHARACTER counts, so a leading `-` takes part (`-123` -> `-,123`).
 */
function groupBy3(text: string, sep: string): string {
  let out = '';
  const n = text.length;
  for (let i = n - 1; i >= 0; i -= 1) {
    out = (text[i] ?? '') + out;
    if ((n - i) % 3 === 0 && i !== 0) out = sep + out;
  }
  return out;
}

/** The integer part of a finite number as the C `%d` of an `int` prints it (truncated toward zero). */
function intText(n: number): string {
  return String(Math.trunc(Number.isFinite(n) ? n : 0));
}

/** `GetNumberFormatEx`: thousands separated by `,` (`1234567` -> `"1,234,567"`). */
export function fmtNumberComma(n: number): string {
  return groupBy3(intText(n), ',');
}

/** `Separator(n, c)` (`__IMPROVED_STATUS`): thousands separated by `sep` (`fmtSeparator(1234567, '.')` = `"1.234.567"`). */
export function fmtSeparator(n: number, sep = '.'): string {
  return groupBy3(intText(n), sep);
}

/**
 * The compact form of the improved status bar: `n >= 1e9` -> `"%.1fb"`, `>= 1e6` -> `"%.1fm"`, `>= 10 000` ->
 * `"%.1fk"` (value / 1000), else the `.`-separated integer.
 */
export function fmtCompact(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}b`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}m`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return fmtSeparator(n, '.');
}
