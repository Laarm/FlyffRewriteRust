// The pure parts of the log page (Plan/25 section 9.8): the lines a view keeps, the filters, and the text of the visible lines.

export interface LogEntry {
  /** Counts the lines received on this page (a stable React key). */
  readonly key: number;
  readonly ts_ms: number;
  /** 1 error .. 5 trace. */
  readonly level: number;
  readonly target: string;
  readonly text: string;
}

/** The page keeps this many lines; older ones fall off the top. */
export const MAX_LINES = 2000;

export const LEVELS = [
  { value: 1, label: 'Error' },
  { value: 2, label: 'Warn' },
  { value: 3, label: 'Info' },
  { value: 4, label: 'Debug' },
  { value: 5, label: 'Trace' },
] as const;

export function levelName(level: number): string {
  return (LEVELS.find((l) => l.value === level)?.label ?? 'Trace').toUpperCase();
}

/** Appends `add` and keeps the newest `max` lines. */
export function appendLines(
  current: readonly LogEntry[],
  add: readonly LogEntry[],
  max: number = MAX_LINES,
): readonly LogEntry[] {
  if (add.length === 0) return current;
  const all = current.length === 0 ? add : [...current, ...add];
  return all.length > max ? all.slice(all.length - max) : all;
}

/** `text` is a case-insensitive substring of the target or the line (empty matches everything). */
export function matchesText(e: LogEntry, text: string): boolean {
  const needle = text.trim().toLowerCase();
  return (
    needle === '' ||
    e.text.toLowerCase().includes(needle) ||
    e.target.toLowerCase().includes(needle)
  );
}

const pad = (n: number, w = 2): string => String(n).padStart(w, '0');

/** `2026-10-23 08:30:12.123 INFO  target  text` (browser zone). */
export function formatLogLine(e: LogEntry): string {
  const d = new Date(e.ts_ms);
  const stamp = `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
  return `${stamp} ${levelName(e.level).padEnd(5)} ${e.target} ${e.text}`;
}

export function visibleText(lines: readonly LogEntry[]): string {
  return lines.map(formatLogLine).join('\n');
}
