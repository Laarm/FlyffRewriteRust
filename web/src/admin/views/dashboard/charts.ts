// The arithmetic of the inline SVG charts (Plan/25 section 9.2: "small inline SVG line charts, no chart library"). Pure and tested.

/** The smallest "nice" number (1, 2 or 5 times a power of ten) that is at least `v`; 1 for anything not positive. */
export function niceCeil(v: number): number {
  if (!Number.isFinite(v) || v <= 0) return 1;
  const exp = Math.floor(Math.log10(v));
  const base = 10 ** exp;
  const f = v / base;
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10;
  return nice * base;
}

export interface YScale {
  readonly max: number;
  /** Tick values from 0 to `max`. */
  readonly ticks: readonly number[];
}

/** The y axis for data whose largest value is `dataMax`: 0 to a nice maximum with 4 intervals. */
export function yScale(dataMax: number, floorMax = 1): YScale {
  const max = niceCeil(Math.max(dataMax, floorMax));
  return { max, ticks: [0, 1, 2, 3, 4].map((i) => (max * i) / 4) };
}

/** Evenly spaced tick times (Unix seconds) from `t0` to `t1`, `count` of them including both ends. */
export function timeTicks(t0: number, t1: number, count = 5): readonly number[] {
  if (count < 2 || t1 <= t0) return [t0];
  return Array.from({ length: count }, (_, i) => Math.round(t0 + ((t1 - t0) * i) / (count - 1)));
}

export interface Box {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

/** The `d` attribute of the polyline through the points (gaps: a `null` value breaks the line). */
export function linePath(
  t: readonly number[],
  values: readonly (number | null)[],
  box: Box,
  t0: number,
  t1: number,
  yMax: number,
): string {
  const span = t1 - t0 || 1;
  let d = '';
  let pen = false;
  for (let i = 0; i < t.length; i += 1) {
    const v = values[i];
    const ti = t[i];
    if (v == null || ti === undefined || !Number.isFinite(v)) {
      pen = false;
      continue;
    }
    const x = box.left + ((ti - t0) / span) * box.width;
    const y = box.top + box.height - (Math.min(v, yMax) / (yMax || 1)) * box.height;
    d += `${pen ? 'L' : 'M'}${x.toFixed(1)},${y.toFixed(1)}`;
    pen = true;
  }
  return d;
}

/** The index of the sample nearest to time `at` (binary search over ascending `t`); -1 for no samples. */
export function nearestIndex(t: readonly number[], at: number): number {
  if (t.length === 0) return -1;
  let lo = 0;
  let hi = t.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((t[mid] ?? 0) < at) lo = mid + 1;
    else hi = mid;
  }
  const prev = lo > 0 ? lo - 1 : lo;
  return Math.abs((t[prev] ?? 0) - at) <= Math.abs((t[lo] ?? 0) - at) ? prev : lo;
}

/** The time under the pointer at `x` pixels inside the plot box. */
export function timeAt(x: number, box: Box, t0: number, t1: number): number {
  const f = Math.min(1, Math.max(0, (x - box.left) / (box.width || 1)));
  return t0 + f * (t1 - t0);
}

export interface Summary {
  readonly last: number;
  readonly max: number;
  readonly avg: number;
}

/** Last, maximum and mean of a series (`null` for an empty one). */
export function summarize(values: readonly number[]): Summary | null {
  if (values.length === 0) return null;
  let max = -Infinity;
  let sum = 0;
  for (const v of values) {
    if (v > max) max = v;
    sum += v;
  }
  return { last: values[values.length - 1] ?? 0, max, avg: sum / values.length };
}
