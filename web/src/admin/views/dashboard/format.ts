// Number and time formats of the dashboard and the server pages (pure; tested).

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** `1_572_864` as `1.5 MB`. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '-';
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit] ?? 'B'}`;
}

/** Bytes per second as `12 KB/s`. */
export function formatRate(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** `3725` seconds as `1h 2m`, `93784` as `1d 2h`, `75` as `1m 15s`. */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const d = Math.floor(s / 86_400);
  const h = Math.floor((s % 86_400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${String(d)}d ${String(h)}h`;
  if (h > 0) return `${String(h)}h ${String(m)}m`;
  if (m > 0) return `${String(m)}m ${String(s % 60)}s`;
  return `${String(s)}s`;
}

/** `12` seconds ago as `12 s ago`; `7500` as `2 h ago`. `nowS` is Unix seconds. */
export function formatAgo(thenS: number, nowS: number): string {
  const d = Math.max(0, nowS - thenS);
  if (d < 60) return `${String(Math.floor(d))} s ago`;
  if (d < 3600) return `${String(Math.floor(d / 60))} min ago`;
  if (d < 86_400) return `${String(Math.floor(d / 3600))} h ago`;
  return `${String(Math.floor(d / 86_400))} d ago`;
}

const pad = (n: number): string => (n < 10 ? `0${String(n)}` : String(n));

/** `HH:MM` of a Unix time in the browser's zone. */
export function formatClock(unixS: number): string {
  const d = new Date(unixS * 1000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `YYYY-MM-DD HH:MM:SS` of a Unix time in the browser's zone. */
export function formatDateTime(unixS: number): string {
  const d = new Date(unixS * 1000);
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** `value / total` as a whole percentage (0 when the total is 0). */
export function percent(value: number, total: number): number {
  return total > 0 ? Math.min(100, Math.round((value / total) * 100)) : 0;
}

/** Microseconds as milliseconds with one decimal (`3100` as `3.1 ms`). */
export function formatMicros(us: number): string {
  return `${(us / 1000).toFixed(1)} ms`;
}
