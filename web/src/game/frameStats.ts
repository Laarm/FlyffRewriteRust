// Fixed-size statistics used by the game loop (Plan/40 section 10, 13.2): a ring of the last frame
// durations (`__flyffDebug.loop().frameTimes(n)`), a sliding one-second event window (steps per second,
// frames per second) and per-section timings (frame budget probes, section 15). Allocation-free in the
// hot path.

/** Ring of the last `capacity` numbers. */
export class FrameTimeRing {
  private readonly values: Float64Array;
  private next = 0;
  private filled = 0;

  constructor(readonly capacity = 600) {
    this.values = new Float64Array(capacity);
  }

  record(ms: number): void {
    this.values[this.next] = ms;
    this.next = (this.next + 1) % this.capacity;
    if (this.filled < this.capacity) this.filled += 1;
  }

  get count(): number {
    return this.filled;
  }

  /** The newest `n` values, oldest first (a copy); `[]` before the first record. */
  last(n: number = this.capacity): number[] {
    const take = Math.max(0, Math.min(Math.floor(n), this.filled));
    const out = new Array<number>(take);
    let idx = (this.next - take + this.capacity * 2) % this.capacity;
    for (let i = 0; i < take; i += 1) {
      out[i] = this.values[idx] ?? 0;
      idx = (idx + 1) % this.capacity;
    }
    return out;
  }
}

/** Sum of the values added during the last `windowMs`, over a bounded ring of (time, value) entries. */
export class SlidingSum {
  private readonly times: Float64Array;
  private readonly amounts: Float64Array;
  private head = 0;
  private size = 0;
  private total = 0;

  constructor(
    private readonly windowMs = 1000,
    private readonly capacity = 1024,
  ) {
    this.times = new Float64Array(capacity);
    this.amounts = new Float64Array(capacity);
  }

  add(nowMs: number, amount: number): void {
    this.evict(nowMs);
    if (this.size === this.capacity) this.dropOldest();
    const idx = (this.head + this.size) % this.capacity;
    this.times[idx] = nowMs;
    this.amounts[idx] = amount;
    this.size += 1;
    this.total += amount;
  }

  /** The sum over `(nowMs - windowMs, nowMs]`. */
  sum(nowMs: number): number {
    this.evict(nowMs);
    return this.total;
  }

  private dropOldest(): void {
    this.total -= this.amounts[this.head] ?? 0;
    this.head = (this.head + 1) % this.capacity;
    this.size -= 1;
  }

  private evict(nowMs: number): void {
    while (this.size > 0 && nowMs - (this.times[this.head] ?? 0) >= this.windowMs)
      this.dropOldest();
    if (this.size === 0) this.total = 0;
  }
}

/** Last and maximum duration per named section of the iteration (the frame budget probes). */
export class SectionTimes<K extends string> {
  private readonly lastMs: Record<string, number> = {};
  private readonly maxMs: Record<string, number> = {};

  constructor(readonly sections: readonly K[]) {
    for (const k of sections) {
      this.lastMs[k] = 0;
      this.maxMs[k] = 0;
    }
  }

  record(section: K, ms: number): void {
    this.lastMs[section] = ms;
    if (ms > (this.maxMs[section] ?? 0)) this.maxMs[section] = ms;
  }

  snapshot(): Record<K, { last: number; max: number }> {
    const out = {} as Record<K, { last: number; max: number }>;
    for (const k of this.sections) out[k] = { last: this.lastMs[k] ?? 0, max: this.maxMs[k] ?? 0 };
    return out;
  }

  resetMax(): void {
    for (const k of this.sections) this.maxMs[k] = 0;
  }
}
