// The CPU decoded cache and the GPU accounting (Plan/40 section 9.3): a table of ref-counted entries with
//
//   * CPU budget 192 MB: zero-ref entries are evicted least-recently-used first when the sum is over budget, and
//     after 30 s of idleness;
//   * GPU budget per quality tier (low 0.75 GB, medium 1 GB, high 1.5 GB): zero-ref entries with GPU bytes are
//     evicted LRU when over budget and after 120 s of idleness; an entry uploaded in the current frame is never
//     evicted (the upload must be able to finish its draw).
//
// The cache does not know what a value is: the manager gives it `dispose`. Time is injected.

import type { AssetError } from './errors';
import type { AssetHandle, HandleState } from './handles';
import type { AssetKey } from './source';
import type { AssetType } from './types';

export const MIB = 1024 * 1024;
/** CPU decoded cache budget (Plan/40 section 9.3, 15). */
export const CPU_BUDGET_BYTES = 192 * MIB;
/** VRAM estimate per quality tier (Plan/01 12.2, 41 R13). */
export const GPU_BUDGET_BYTES = { low: 768 * MIB, medium: 1024 * MIB, high: 1536 * MIB } as const;
/** Idle time after which a zero-ref CPU entry is evicted. */
export const CPU_IDLE_MS = 30_000;
/** Idle time after which a zero-ref entry with GPU resources is evicted. */
export const GPU_IDLE_MS = 120_000;

/** One cached asset: the state of a load and its accounting. */
export interface CacheEntry<T = unknown> {
  /** `<type>|<keyString>`. */
  readonly id: string;
  readonly keyStr: string;
  readonly key: AssetKey;
  readonly type: AssetType<T>;
  refs: number;
  state: HandleState;
  value: T | undefined;
  error: AssetError | null;
  cpuBytes: number;
  gpuBytes: number;
  /** Last acquire/touch time (LRU order). */
  lastUsed: number;
  /** When `refs` dropped to 0 (null while referenced). */
  idleSince: number | null;
  /** The frame in which GPU bytes were last added (never evicted in it). */
  uploadedFrame: number;
  /** World generation (null = never cancelled by a bump). */
  generation: number | null;
  prio: number;
  distance: number;
  /** Cancels the load (the fetch and the decode job). */
  abort: AbortController;
  promise: Promise<T>;
  deps: AssetHandle<unknown>[];
  /** Set once the entry left the table (a late result must be dropped). */
  disposed: boolean;
  /** WebGL context lost: the GPU resources of this entry are gone and must be uploaded again (Plan/40 section 12.4). */
  reupload?: boolean;
}

export interface SweepResult {
  readonly evicted: number;
  readonly cpuBytes: number;
  readonly gpuBytes: number;
}

export interface CacheBudgets {
  cpu: number;
  gpu: number;
  cpuIdleMs: number;
  gpuIdleMs: number;
}

export class AssetCache {
  private readonly table = new Map<string, CacheEntry>();
  private cpu = 0;
  private gpu = 0;

  get size(): number {
    return this.table.size;
  }
  get cpuBytes(): number {
    return this.cpu;
  }
  get gpuBytes(): number {
    return this.gpu;
  }

  get(id: string): CacheEntry | undefined {
    return this.table.get(id);
  }

  set(entry: CacheEntry): void {
    this.table.set(entry.id, entry);
    this.cpu += entry.cpuBytes;
    this.gpu += entry.gpuBytes;
  }

  /** Removes an entry and its accounting; `false` when it was not in the table. */
  delete(entry: CacheEntry): boolean {
    if (this.table.get(entry.id) !== entry) return false;
    this.table.delete(entry.id);
    this.cpu -= entry.cpuBytes;
    this.gpu -= entry.gpuBytes;
    return true;
  }

  /** Changes the byte counts of a resident entry. */
  account(entry: CacheEntry, cpuBytes: number, gpuBytes: number): void {
    if (this.table.get(entry.id) === entry) {
      this.cpu += cpuBytes - entry.cpuBytes;
      this.gpu += gpuBytes - entry.gpuBytes;
    }
    entry.cpuBytes = cpuBytes;
    entry.gpuBytes = gpuBytes;
  }

  values(): IterableIterator<CacheEntry> {
    return this.table.values();
  }

  /**
   * Picks the entries to evict: idle ones past their limit, then the least recently used zero-ref entries while a
   * budget is exceeded. The caller removes them (`delete` + dispose). `frame` protects entries uploaded in it.
   */
  victims(now: number, frame: number, budgets: CacheBudgets): CacheEntry[] {
    const out = new Set<CacheEntry>();
    const idle: CacheEntry[] = [];
    for (const e of this.table.values()) {
      if (e.refs > 0 || e.state === 'loading') continue;
      idle.push(e);
      const since = e.idleSince ?? now;
      const limit = e.gpuBytes > 0 ? budgets.gpuIdleMs : budgets.cpuIdleMs;
      if (now - since >= limit && e.uploadedFrame !== frame) out.add(e);
    }
    idle.sort((a, b) => a.lastUsed - b.lastUsed);
    let cpu = this.cpu;
    let gpu = this.gpu;
    for (const e of out) {
      cpu -= e.cpuBytes;
      gpu -= e.gpuBytes;
    }
    for (const e of idle) {
      if (cpu <= budgets.cpu && gpu <= budgets.gpu) break;
      if (out.has(e) || e.uploadedFrame === frame) continue;
      const helpsCpu = cpu > budgets.cpu && e.cpuBytes > 0;
      const helpsGpu = gpu > budgets.gpu && e.gpuBytes > 0;
      if (!helpsCpu && !helpsGpu) continue;
      out.add(e);
      cpu -= e.cpuBytes;
      gpu -= e.gpuBytes;
    }
    return [...out];
  }
}
