// Small pure helpers of the siege windows: the legacy `printf` conversions of the texts (`TID_*` templates with `%s`, `%d`, `%.2d`), the `GetStrCut` cut of names, the padded numbers of the
// rows and the countdown of the offer state. `features/common/text.ts` has the same template routine for the features; `ui/` may not import it, so the few conversions the windows need are here.

/** One printf conversion of a legacy template: `%s %d %u %ld %I64d %c %i` with flags / width / precision (accepted, only the width and `0` matter for numbers), and `%%`. */
const CONVERSION = /%(?:%|[-+ #0]*\d*(?:\.\d+)?(?:I64|ll|l|h)?[sducfxXi])/g;

/** `str.Format( template, args )`: the i-th conversion takes `args[i]`; a missing argument is `''`, `%%` is one `%`. */
export function fmt(template: string, args: readonly (string | number | bigint)[]): string {
  let i = 0;
  return template.replace(CONVERSION, (m) => {
    if (m === '%%') return '%';
    const a = args[i];
    i += 1;
    return a === undefined ? '' : String(a);
  });
}

/** `"%.2d"`: at least two digits (zero padded). */
export function pad2(n: number): string {
  return String(Math.trunc(n)).padStart(2, '0');
}

/** `"%2d"` / `"%3d"`: right-aligned in a field of `width` blanks. */
export function padLeft(n: number | string, width: number): string {
  return String(n).padStart(width, ' ');
}

/**
 * `GetStrCut( str, buf, n )` plus the `"..."` of the lists: a name of `n` characters or more is cut to `n` characters and gets the dots (KEEP: a name of exactly `n` characters also
 * gets them; the original compares `n <= GetStrLen( str )`).
 */
export function cutName(name: string, n: number): string {
  const chars = Array.from(name);
  return chars.length >= n ? `${chars.slice(0, n).join('')}...` : name;
}

/** `"%.Ns"`: the first `n` characters. */
export function head(text: string, n: number): string {
  return Array.from(text).slice(0, n).join('');
}

/** The pieces of a `CTimeSpan` of `seconds`: days, hours (0-23), minutes and seconds (0-59). */
export function timeSpan(seconds: number): {
  days: number;
  hours: number;
  minutes: number;
  seconds: number;
} {
  const s = Math.max(0, Math.floor(seconds));
  return {
    days: Math.floor(s / 86400),
    hours: Math.floor(s / 3600) % 24,
    minutes: Math.floor(s / 60) % 60,
    seconds: s % 60,
  };
}

/** `"%.2d:%.2d:%.2d"` of `CTimeSpan.GetHours / GetMinutes / GetSeconds` (the days are not shown, as in the original). */
export function clock(seconds: number): string {
  const t = timeSpan(seconds);
  return `${pad2(t.hours)}:${pad2(t.minutes)}:${pad2(t.seconds)}`;
}
