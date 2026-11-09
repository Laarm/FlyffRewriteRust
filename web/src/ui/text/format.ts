// Number and time formatting of the UI texts (Plan/42 §8.4; `GetNumberFormatEx`, `_Common/xUtil.cpp`).

/**
 * `GetNumberFormatEx(szNumber)`: inserts a `,` before every third CHARACTER counted from the right, whatever the
 * characters are (the original does not look at digits): `"1234567"` -> `"1,234,567"`. KEEP-QUIRK: a negative number
 * whose digit count is a multiple of three gets a comma after the sign (`"-123"` -> `"-,123"`).
 */
export function groupThousands(text: string): string {
  const chars = Array.from(text);
  const n = chars.length;
  let out = '';
  for (let i = n - 1; i >= 0; i -= 1) {
    out = (chars[i] ?? '') + out;
    if ((n - i) % 3 === 0 && i !== 0) out = `,${out}`;
  }
  return out;
}

/** `WSS_MONEY`: the amount with thousands separators (`1234567` -> `1,234,567`). */
export function formatMoney(value: number | string): string {
  return groupThousands(typeof value === 'number' ? String(Math.trunc(value)) : value);
}

/** `mm:ss` of a duration in seconds (minutes are not capped: 3725 s -> `62:05`); negative values count as 0. */
export function formatMmSs(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const mm = Math.floor(s / 60);
  const ss = s % 60;
  return `${String(mm).padStart(2, '0')}:${String(ss).padStart(2, '0')}`;
}
