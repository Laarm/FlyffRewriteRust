// Small pure formatters of the players views (dates, durations, penya, authority letters). No clock is read here: every function that
// needs "now" takes it as an argument, so renders stay pure and tests stay exact.

const two = (n: number): string => String(n).padStart(2, '0');

/** `2026-10-23 14:05` in the browser's time zone, `-` for 0 / missing. */
export function formatDateTime(unixSeconds: number | null | undefined): string {
  if (unixSeconds === null || unixSeconds === undefined || unixSeconds <= 0) return '-';
  const d = new Date(unixSeconds * 1000);
  return `${String(d.getFullYear())}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** `5 s`, `3 min`, `2 h 10 min`, `4 d 3 h`: the two largest units of a span in seconds. */
export function formatSpan(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  if (s < 60) return `${String(s)} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${String(m)} min`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 === 0 ? `${String(h)} h` : `${String(h)} h ${String(m % 60)} min`;
  const d = Math.floor(h / 24);
  return h % 24 === 0 ? `${String(d)} d` : `${String(d)} d ${String(h % 24)} h`;
}

/** `3 min ago` / `in 2 h`, `-` for a missing time. */
export function formatAgo(nowSeconds: number, unixSeconds: number | null | undefined): string {
  if (unixSeconds === null || unixSeconds === undefined || unixSeconds <= 0) return '-';
  const diff = nowSeconds - unixSeconds;
  if (Math.abs(diff) < 5) return 'now';
  return diff >= 0 ? `${formatSpan(diff)} ago` : `in ${formatSpan(-diff)}`;
}

/** Groups thousands: `1 234 567`. */
export function formatNumber(n: number | bigint): string {
  const text = n.toString();
  const negative = text.startsWith('-');
  const digits = negative ? text.slice(1) : text;
  const grouped = digits.replace(/\B(?=(\d{3})+(?!\d))/g, ' ');
  return negative ? `-${grouped}` : grouped;
}

/** `45 min` of play time, `3 d 4 h` and so on. */
export function formatPlayTime(seconds: number): string {
  return seconds <= 0 ? '-' : formatSpan(seconds);
}

/** The exp of a level in percent with one decimal (`exp_permille` is 0..1000). */
export function formatPermille(permille: number | null | undefined): string {
  if (permille === null || permille === undefined) return '-';
  return `${(permille / 10).toFixed(1)} %`;
}

/** `[x, y, z]` of a position in whole metres. */
export function formatPos(x: number, z: number): string {
  return `${String(Math.round(x))}, ${String(Math.round(z))}`;
}

/** The text of an authority letter for a badge (`F` player ... `P` administrator). */
export function authorityName(letter: string): string {
  switch (letter) {
    case 'F':
      return 'Player';
    case 'G':
      return 'GM (G)';
    case 'L':
      return 'GM (L)';
    case 'M':
      return 'GM (M)';
    case 'N':
      return 'GM (N)';
    case 'P':
      return 'Admin (P)';
    default:
      return letter;
  }
}

/** `true` for a letter that is staff (`L` and above). */
export function isStaffLetter(letter: string): boolean {
  return 'LMNP'.includes(letter) && letter.length === 1;
}

/** `Preset: text` of a reason, the form the older routes (kick, ban) take as one string. */
export function reasonLine(
  reason: { readonly preset?: string | null; readonly text?: string | null } | undefined,
): string | undefined {
  if (!reason) return undefined;
  const preset = reason.preset?.trim() ?? '';
  const text = reason.text?.trim() ?? '';
  if (preset !== '' && text !== '') return `${preset}: ${text}`;
  const line = preset !== '' ? preset : text;
  return line === '' ? undefined : line;
}
