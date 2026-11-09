// The per-frame GPU upload budget queue and VRAM accounting (Plan/40 section 9.6 / WP-CLIENT-22 interface,
// Plan/41 sections 22.2-22.3). The renderer's texture and geometry uploads are `UploadJob`s whose `run()` does
// the actual three.js/GL work; this queue decides how many run per frame:
//
//   * game budget 4 MB / 3 ms per frame, loading budget 16 MB / 12 ms, hidden tab: no uploads;
//   * order: `Prio` class, then the requester's distance, then FIFO;
//   * progress guarantee: at least one job runs per frame even if it is larger than the budget.
//
// `GpuAccounting` tracks the resident GPU bytes per key (textures, geometry) for the budget and the probe.

import { Prio } from '../../workers/priority';

export { Prio };

/** One unit of GPU work. */
export interface UploadJob {
  readonly key: string;
  readonly prio: Prio;
  /** Lower first inside a class (the requester's distance). */
  readonly distance?: number;
  /** GPU bytes the job uploads (for the budget). */
  readonly bytes: number;
  run(): void;
}

export type UploadMode = 'game' | 'loading' | 'hidden';

/** The budgets of Plan/40 section 9.6. */
export const UPLOAD_BUDGETS: Readonly<Record<UploadMode, { bytes: number; ms: number }>> = {
  game: { bytes: 4 * 1024 * 1024, ms: 3 },
  loading: { bytes: 16 * 1024 * 1024, ms: 12 },
  hidden: { bytes: 0, ms: 0 },
};

interface Queued extends UploadJob {
  readonly seq: number;
}

export interface UploadFrameResult {
  readonly jobs: number;
  readonly bytes: number;
  readonly ms: number;
  readonly pending: number;
}

/** `runFrame` of one iteration, the numbers of the last frame stay readable for the stats. */
export class UploadQueue {
  private readonly queue: Queued[] = [];
  private seq = 0;
  private dirty = false;
  mode: UploadMode = 'game';
  /** Numbers of the last `runFrame`. */
  last: UploadFrameResult = { jobs: 0, bytes: 0, ms: 0, pending: 0 };
  /** Total jobs ever run. */
  completed = 0;
  /** Jobs whose `run()` threw. */
  failed = 0;

  constructor(private readonly clock: () => number = () => performance.now()) {}

  enqueue(job: UploadJob): void {
    this.queue.push({ ...job, seq: this.seq++ });
    this.dirty = true;
  }

  /** Removes every queued job of `key` (the resource was released before its upload ran). */
  cancel(key: string): number {
    let n = 0;
    for (let i = this.queue.length - 1; i >= 0; i--) {
      if (this.queue[i]?.key === key) {
        this.queue.splice(i, 1);
        n++;
      }
    }
    return n;
  }

  get pending(): number {
    return this.queue.length;
  }

  get pendingBytes(): number {
    let n = 0;
    for (const j of this.queue) n += j.bytes;
    return n;
  }

  private sort(): void {
    if (!this.dirty) return;
    this.queue.sort(
      (a, b) => a.prio - b.prio || (a.distance ?? 0) - (b.distance ?? 0) || a.seq - b.seq,
    );
    this.dirty = false;
  }

  /**
   * Runs queued jobs in priority order until the byte or time budget is spent (always at least one job unless
   * the mode is `hidden`). `budgetBytes`/`budgetMs` default to the budget of the current mode.
   */
  runFrame(budgetBytes?: number, budgetMs?: number): UploadFrameResult {
    const budget = UPLOAD_BUDGETS[this.mode];
    const maxBytes = budgetBytes ?? budget.bytes;
    const maxMs = budgetMs ?? budget.ms;
    const t0 = this.clock();
    let jobs = 0;
    let bytes = 0;
    if (this.mode !== 'hidden' || budgetBytes !== undefined) {
      this.sort();
      while (this.queue.length > 0) {
        const next = this.queue[0];
        if (!next) break;
        if (jobs > 0 && (bytes + next.bytes > maxBytes || this.clock() - t0 >= maxMs)) break;
        this.queue.shift();
        try {
          next.run();
        } catch {
          this.failed++;
        }
        jobs++;
        bytes += next.bytes;
        this.completed++;
      }
    }
    this.last = { jobs, bytes, ms: this.clock() - t0, pending: this.queue.length };
    return this.last;
  }
}

/** Resident GPU bytes per key. */
export class GpuAccounting {
  private readonly bytes = new Map<string, number>();
  private sum = 0;

  /** Records (or replaces) the bytes of `key`. */
  set(key: string, n: number): void {
    this.sum += n - (this.bytes.get(key) ?? 0);
    this.bytes.set(key, n);
  }

  remove(key: string): void {
    const n = this.bytes.get(key);
    if (n === undefined) return;
    this.sum -= n;
    this.bytes.delete(key);
  }

  /** WebGL context loss: every GPU resource is gone (Plan/40 section 12.4, Plan/41 section 4.2). */
  reset(): void {
    this.bytes.clear();
    this.sum = 0;
  }

  get totalBytes(): number {
    return this.sum;
  }

  get totalMb(): number {
    return this.sum / (1024 * 1024);
  }

  get count(): number {
    return this.bytes.size;
  }

  has(key: string): boolean {
    return this.bytes.has(key);
  }
}
