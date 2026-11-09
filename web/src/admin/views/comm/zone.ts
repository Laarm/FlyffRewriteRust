// Times in the server's time zone (Plan/25 section 8.1: the daily time of a notice is `HH:MM` in the zone of the server, and the
// operator should not have to translate between the zone of the browser and the zone of the server). Pure functions over
// `Intl.DateTimeFormat` with an IANA zone name; an unknown name falls back to UTC.

export interface WallParts {
  readonly year: number;
  readonly month: number;
  readonly day: number;
  readonly hour: number;
  readonly minute: number;
}

const formatters = new Map<string, Intl.DateTimeFormat>();

function formatter(zone: string): Intl.DateTimeFormat {
  const known = formatters.get(zone);
  if (known) return known;
  const options: Intl.DateTimeFormatOptions = {
    hourCycle: 'h23',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  };
  let made: Intl.DateTimeFormat;
  try {
    made = new Intl.DateTimeFormat('en-US', { ...options, timeZone: zone });
  } catch {
    made = new Intl.DateTimeFormat('en-US', { ...options, timeZone: 'UTC' });
  }
  formatters.set(zone, made);
  return made;
}

interface Full extends WallParts {
  readonly second: number;
}

function fullParts(unix: number, zone: string): Full {
  const v: Record<string, number> = {};
  for (const p of formatter(zone).formatToParts(new Date(unix * 1000))) {
    if (p.type !== 'literal') v[p.type] = Number(p.value);
  }
  return {
    year: v['year'] ?? 1970,
    month: v['month'] ?? 1,
    day: v['day'] ?? 1,
    hour: (v['hour'] ?? 0) % 24,
    minute: v['minute'] ?? 0,
    second: v['second'] ?? 0,
  };
}

/** The wall-clock reading of an instant in `zone`. */
export function partsInZone(unix: number, zone: string): WallParts {
  const p = fullParts(unix, zone);
  return { year: p.year, month: p.month, day: p.day, hour: p.hour, minute: p.minute };
}

/** The offset of `zone` from UTC at `unix`, seconds east. */
export function offsetSeconds(unix: number, zone: string): number {
  const p = fullParts(unix, zone);
  const asUtc = Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute, p.second) / 1000;
  return Math.round(asUtc - unix);
}

/** The instant at which `zone` reads `parts` (a time in a spring-forward gap lands just after it; an ambiguous one takes the later reading). */
export function unixFromParts(parts: WallParts, zone: string): number {
  const asUtc = Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour, parts.minute) / 1000;
  let t = asUtc - offsetSeconds(asUtc, zone);
  t = asUtc - offsetSeconds(t, zone);
  return t;
}

const two = (n: number): string => String(n).padStart(2, '0');

/** `2026-10-23T18:30`: the value of an `<input type="datetime-local">` showing `unix` in `zone`. */
export function toInputValue(unix: number, zone: string): string {
  const p = partsInZone(unix, zone);
  return `${String(p.year).padStart(4, '0')}-${two(p.month)}-${two(p.day)}T${two(p.hour)}:${two(p.minute)}`;
}

const INPUT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/;

/** The instant an `<input type="datetime-local">` value means in `zone`; `null` for an empty or malformed value. */
export function fromInputValue(text: string, zone: string): number | null {
  const m = INPUT.exec(text.trim());
  if (!m) return null;
  const [year, month, day, hour, minute] = m.slice(1).map(Number);
  if (
    year === undefined ||
    month === undefined ||
    day === undefined ||
    hour === undefined ||
    minute === undefined ||
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > 31 ||
    hour > 23 ||
    minute > 59
  ) {
    return null;
  }
  return unixFromParts({ year, month, day, hour, minute }, zone);
}

/** `2026-10-23 18:30` in `zone`. */
export function formatZoned(unix: number, zone: string): string {
  return toInputValue(unix, zone).replace('T', ' ');
}

/** `UTC+2`, `UTC-3:30`, `UTC`. */
export function formatOffset(offsetS: number): string {
  if (offsetS === 0) return 'UTC';
  const sign = offsetS < 0 ? '-' : '+';
  const abs = Math.abs(offsetS);
  const h = Math.floor(abs / 3600);
  const m = Math.floor((abs % 3600) / 60);
  return `UTC${sign}${String(h)}${m === 0 ? '' : `:${two(m)}`}`;
}

/** `in 3 h 12 min`, `in 40 s`, `now`: how far `unix` is from `now`. */
export function formatUntil(unix: number, now: number): string {
  const d = unix - now;
  if (d <= 0) return 'now';
  if (d < 90) return `in ${String(d)} s`;
  const min = Math.round(d / 60);
  if (min < 90) return `in ${String(min)} min`;
  const h = Math.floor(min / 60);
  const rest = min % 60;
  if (h < 48) return `in ${String(h)} h${rest === 0 ? '' : ` ${String(rest)} min`}`;
  return `in ${String(Math.round(h / 24))} days`;
}
