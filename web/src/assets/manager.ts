// The asset manager (Plan/40 section 9.3, WP-CLIENT-09): ref-counted loads of typed assets.
//
//   acquire(type, key, prio, distance)  ->  AssetHandle   (one load per type+key; a second acquire joins it)
//
// A load is: fetch through the `AssetSource` (priority, distance and generation travel with it), decode in the worker
// pool (the job kind comes from the `AssetType`), `toValue`, accounting, optional GPU upload and dependencies. The
// cache keeps the value while handles exist, then for 30 s (CPU) / 120 s (GPU resources) and evicts least-recently-
// used entries when a budget is exceeded (`cache.ts`). Failures: a missing file (404, no URL, a decoder error) is
// final for the session and logged once per key (KQ-30); a network or 5xx failure is retried by the next `acquire`.

import { DecodeFailedError, JobCancelledError } from '../workers/pool';
import type { JobHandle, JobSpec } from '../workers/pool';
import type { Prio } from '../workers/priority';
import type { WorkerJobKind } from '../workers/protocol';
import {
  AssetCache,
  CPU_BUDGET_BYTES,
  CPU_IDLE_MS,
  GPU_BUDGET_BYTES,
  GPU_IDLE_MS,
  type CacheEntry,
  type SweepResult,
} from './cache';
import { AssetError } from './errors';
import { EntryHandle, type AssetHandle, type HandleOwner } from './handles';
import { keyString, type AssetKey, type AssetSource } from './source';
import type { AssetCaps, AssetType, DecodeSpec } from './types';
import type { UploadJob } from './uploads';

/** The slice of `WorkerPool` the manager uses (a fake implements it in tests). */
export interface DecodePool {
  submit<K extends WorkerJobKind>(spec: JobSpec<K>): JobHandle<K>;
  setGeneration?(generation: number): void;
}

export interface AssetStats {
  /** Network requests on the wire (the source's scheduler). */
  readonly inflight: number;
  /** Network requests waiting for a slot or for a retry. */
  readonly queued: number;
  /** Keys that failed for good (capped at 100, newest last). */
  readonly failed: readonly string[];
  readonly cpuBytes: number;
  readonly gpuBytes: number;
  readonly uploadsPending: number;
  /** Entries whose GPU resources were lost with the context and are not uploaded again yet. */
  readonly reuploadPending: number;
  /** Entries in the cache table. */
  readonly entries: number;
  readonly generation: number;
}

export interface AcquireOptions {
  /**
   * World-scoped: the load carries the current generation and is cancelled (queued) or dropped (in flight) by
   * `bumpGeneration()` (Plan/40 section 8.4). Default `false` (UI, boot and audio assets are not world-scoped).
   */
  readonly scoped?: boolean;
}

export interface AssetManager {
  acquire<T>(
    type: AssetType<T>,
    key: AssetKey,
    prio: Prio,
    distance?: number,
    options?: AcquireOptions,
  ): AssetHandle<T>;
  /** The requester's distance changed: re-ranks the fetch and the decode job while they are queued. */
  setDistance(handle: AssetHandle<unknown>, distance: number): void;
  /** One-shot fetch without caching (bundles, `FLMT`/`FLMB`/`FLYK`, `/dev` files). */
  fetchBytes(key: AssetKey, prio: Prio, options?: FetchBytesOptions): Promise<ArrayBuffer>;
  readonly generation: number;
  /** A world change, leave or re-attach: older queued requests are cancelled, older in-flight results dropped. */
  bumpGeneration(): void;
  /** The frame counter of the loop; entries that got GPU bytes in the current frame are not evicted. */
  setFrame(frame: number): void;
  /** 41 reports the GPU bytes an upload made resident for this handle's asset. */
  accountGpu(handle: AssetHandle<unknown>, bytes: number): void;
  /** The CPU copy of a decoded asset was dropped after the upload (`cpuKeep = false`). */
  dropCpu(handle: AssetHandle<unknown>): void;
  setGpuBudget(bytes: number): void;
  /**
   * The WebGL context was lost (Plan/40 section 12.4): every GPU-backed entry is marked for re-upload and `gpuBytes` is 0.
   * Returns the number of entries marked.
   */
  contextLost(): number;
  /**
   * The context is back: the marked entries queue their uploads again (the decoded value is still held; an entry whose type
   * has no `upload` was re-created by its owner, 41, and is only unmarked). Returns the number of uploads queued.
   */
  contextRestored(): number;
  /** Evicts idle and over-budget entries. */
  sweep(nowMs?: number): SweepResult;
  /** Runs `sweep` every `intervalMs` (default 5 s); returns the stop function. */
  startSweeper(intervalMs?: number): () => void;
  readonly stats: AssetStats;
  /** Cancels every load and frees every value. */
  dispose(): void;
}

export interface FetchBytesOptions {
  readonly signal?: AbortSignal;
  readonly distance?: number;
  readonly scoped?: boolean;
}

export interface AssetManagerOptions {
  readonly source: AssetSource;
  /** Decode pool; `null` when no worker exists (types with `decode: null` still work). */
  readonly pool?: DecodePool | null;
  readonly caps?: AssetCaps;
  readonly now?: () => number;
  readonly cpuBudget?: number;
  readonly gpuBudget?: number;
  readonly cpuIdleMs?: number;
  readonly gpuIdleMs?: number;
  /** Where decoded values with an `upload` go (`render/gl/uploadQueue`). */
  readonly uploads?: { enqueue(job: UploadJob): void; readonly pending?: number };
  /** Network numbers for the stats. */
  readonly sourceStats?: () => { inflight: number; queued: number };
  readonly log?: { warn(scope: string, message: string): void };
  /** A scoped request was finished or failed: lets the preparing tracker know. */
  readonly onFailure?: (error: AssetError) => void;
}

const FAILED_CAP = 100;

function noop(): void {
  // a rejection nobody awaits (a cancelled prefetch) must not surface as an unhandled rejection
}

function toAssetError(error: unknown, key: string): AssetError {
  if (error instanceof AssetError) return error;
  if (error instanceof JobCancelledError) return new AssetError('aborted', key, error.message);
  if (error instanceof DecodeFailedError) {
    return new AssetError('decode', key, error.message, undefined, error);
  }
  if (error instanceof DOMException && error.name === 'AbortError') {
    return new AssetError('aborted', key, 'aborted');
  }
  return new AssetError(
    'decode',
    key,
    error instanceof Error ? error.message : String(error),
    undefined,
    error,
  );
}

/** `byteLength` of the blob or buffer inside a decoded value (a rough CPU estimate for the budget). */
function defaultCpuBytes(value: unknown): number {
  if (value instanceof ArrayBuffer) return value.byteLength;
  if (typeof value === 'object' && value !== null) {
    const blob = (value as { blob?: unknown }).blob;
    if (blob instanceof ArrayBuffer) return blob.byteLength;
  }
  return 0;
}

export function createAssetManager(options: AssetManagerOptions): AssetManager {
  return new AssetManagerImpl(options);
}

class AssetManagerImpl implements AssetManager, HandleOwner<CacheEntry> {
  private readonly cache = new AssetCache();
  private readonly finals = new Map<string, AssetError>();
  private readonly failedKeys: string[] = [];
  private readonly handleEntry = new WeakMap<object, CacheEntry>();
  private readonly now: () => number;
  private readonly caps: AssetCaps;
  private gpuBudget: number;
  private frameNo = 0;
  private gen = 0;
  private sweeper: ReturnType<typeof setInterval> | null = null;

  constructor(private readonly o: AssetManagerOptions) {
    this.now = o.now ?? (() => performance.now());
    this.caps = o.caps ?? { s3tc: true };
    this.gpuBudget = o.gpuBudget ?? GPU_BUDGET_BYTES.medium;
  }

  get generation(): number {
    return this.gen;
  }

  get stats(): AssetStats {
    const net = this.o.sourceStats?.() ?? { inflight: 0, queued: 0 };
    return {
      inflight: net.inflight,
      queued: net.queued,
      failed: [...this.failedKeys],
      cpuBytes: this.cache.cpuBytes,
      gpuBytes: this.cache.gpuBytes,
      uploadsPending: this.o.uploads?.pending ?? 0,
      reuploadPending: this.countReupload(),
      entries: this.cache.size,
      generation: this.gen,
    };
  }

  // ---- acquire / release --------------------------------------------------------------------------------

  acquire<T>(
    type: AssetType<T>,
    key: AssetKey,
    prio: Prio,
    distance = 0,
    options: AcquireOptions = {},
  ): AssetHandle<T> {
    const keyStr = keyString(key);
    const id = `${type.name}|${keyStr}`;
    const now = this.now();
    const final = this.finals.get(id);
    if (final) return this.handleOf(this.failedEntry(id, keyStr, key, type, final, now));
    let entry = this.cache.get(id) as CacheEntry<T> | undefined;
    if (entry?.state === 'failed') {
      // a transient failure (network, 5xx) with other holders still around: try again with a new load
      this.cache.delete(entry);
      entry = undefined;
    }
    if (entry) {
      entry.refs += 1;
      entry.lastUsed = now;
      entry.idleSince = null;
      if (entry.state === 'loading' && (prio < entry.prio || distance < entry.distance)) {
        entry.prio = Math.min(entry.prio, prio);
        entry.distance = Math.min(entry.distance, distance);
        this.o.source.setDistance?.(key, entry.distance);
      }
      if (options.scoped !== true) entry.generation = null;
      return this.handleOf(entry);
    }
    entry = this.start(
      type,
      key,
      keyStr,
      id,
      prio,
      distance,
      options.scoped === true ? this.gen : null,
      now,
    );
    return this.handleOf(entry);
  }

  private handleOf<T>(entry: CacheEntry<T>): AssetHandle<T> {
    const handle = new EntryHandle<T, CacheEntry<T>>(entry, this);
    this.handleEntry.set(handle, entry);
    return handle;
  }

  releaseEntry(entry: CacheEntry): void {
    entry.refs = Math.max(0, entry.refs - 1);
    if (entry.refs > 0) return;
    entry.idleSince = this.now();
    if (entry.state === 'loading') {
      // nobody waits for it any more: cancel the fetch and the decode (Plan/40 section 8.4)
      this.discard(entry);
    } else if (entry.state === 'failed') {
      this.discard(entry);
    }
  }

  setDistance(handle: AssetHandle<unknown>, distance: number): void {
    const entry = this.handleEntry.get(handle);
    if (entry?.state !== 'loading') return;
    entry.distance = distance;
    this.o.source.setDistance?.(entry.key, distance);
  }

  fetchBytes(key: AssetKey, prio: Prio, options: FetchBytesOptions = {}): Promise<ArrayBuffer> {
    const signal = options.signal ?? new AbortController().signal;
    return this.o.source.fetch(key, prio, signal, {
      distance: options.distance ?? 0,
      generation: options.scoped === true ? this.gen : null,
    });
  }

  // ---- loading ------------------------------------------------------------------------------------------

  private start<T>(
    type: AssetType<T>,
    key: AssetKey,
    keyStr: string,
    id: string,
    prio: Prio,
    distance: number,
    generation: number | null,
    now: number,
  ): CacheEntry<T> {
    const abort = new AbortController();
    const entry = {
      id,
      keyStr,
      key,
      type,
      refs: 1,
      state: 'loading',
      value: undefined,
      error: null,
      cpuBytes: 0,
      gpuBytes: 0,
      lastUsed: now,
      idleSince: null,
      uploadedFrame: -1,
      generation,
      prio,
      distance,
      abort,
      deps: [],
      disposed: false,
    } as unknown as CacheEntry<T>;
    this.cache.set(entry);
    entry.promise = this.load(entry).then(
      (value) => this.loaded(entry, value),
      (error: unknown) => {
        throw this.failed(entry, toAssetError(error, keyStr));
      },
    );
    entry.promise.catch(noop);
    return entry;
  }

  private failedEntry<T>(
    id: string,
    keyStr: string,
    key: AssetKey,
    type: AssetType<T>,
    error: AssetError,
    now: number,
  ): CacheEntry<T> {
    const promise = Promise.reject<T>(error);
    promise.catch(noop);
    return {
      id,
      keyStr,
      key,
      type,
      refs: 1,
      state: 'failed',
      value: undefined,
      error,
      cpuBytes: 0,
      gpuBytes: 0,
      lastUsed: now,
      idleSince: null,
      uploadedFrame: -1,
      generation: null,
      prio: 0,
      distance: 0,
      abort: new AbortController(),
      promise,
      deps: [],
      disposed: true,
    };
  }

  private decodeSpec<T>(type: AssetType<T>, key: AssetKey): DecodeSpec | null {
    if (type.decode === null) return null;
    if (typeof type.decode === 'function') return type.decode(key, this.caps);
    return { kind: type.decode };
  }

  private stale(entry: CacheEntry): boolean {
    return entry.disposed || (entry.generation !== null && entry.generation < this.gen);
  }

  private async load<T>(entry: CacheEntry<T>): Promise<T> {
    const { type, key, keyStr } = entry;
    const { signal } = entry.abort;
    const bytes = await this.o.source.fetch(key, entry.prio as Prio, signal, {
      distance: entry.distance,
      generation: entry.generation,
    });
    if (this.stale(entry)) throw new AssetError('aborted', keyStr, 'stale result dropped');
    const spec = this.decodeSpec(type, key);
    let result: unknown = bytes;
    if (spec !== null) {
      const pool = this.o.pool;
      if (!pool) throw new AssetError('decode', keyStr, 'no decode worker pool');
      const job = pool.submit({
        kind: spec.kind,
        bytes,
        opts: spec.opts,
        prio: entry.prio as Prio,
        distance: entry.distance,
        generation: entry.generation ?? undefined,
        signal,
      } as JobSpec);
      result = await job.promise;
      if (this.stale(entry)) throw new AssetError('aborted', keyStr, 'stale result dropped');
    }
    return type.toValue ? type.toValue(result, key) : (result as T);
  }

  private loaded<T>(entry: CacheEntry<T>, value: T): T {
    const { type, key } = entry;
    entry.value = value;
    entry.state = 'ready';
    this.cache.account(entry, type.cpuBytes?.(value) ?? defaultCpuBytes(value), entry.gpuBytes);
    if (type.dependencies) {
      for (const d of type.dependencies(value)) {
        entry.deps.push(
          this.acquire(d.type, d.key, entry.prio as Prio, entry.distance, {
            scoped: entry.generation !== null,
          }),
        );
      }
    }
    const uploads = this.o.uploads;
    if (type.upload && uploads) {
      const job = type.upload(value, key);
      if (job) {
        uploads.enqueue({
          ...job,
          run: () => {
            if (entry.disposed) return;
            job.run();
            this.accountGpu(this.handleOf(entry), job.bytes);
            if (!type.cpuKeep) this.cache.account(entry, 0, entry.gpuBytes);
          },
        });
      }
    }
    return value;
  }

  private failed(entry: CacheEntry, error: AssetError): AssetError {
    entry.state = 'failed';
    entry.error = error;
    if (error.reason === 'aborted') {
      this.discard(entry);
      return error;
    }
    if (error.missing || error.reason === 'decode') {
      // final for the session: never retried, logged once per key (KQ-30)
      if (!this.finals.has(entry.id)) {
        this.finals.set(entry.id, error);
        this.o.log?.warn('assets', error.message);
      }
    } else {
      this.o.log?.warn('assets', error.message);
    }
    if (!this.failedKeys.includes(entry.keyStr)) {
      this.failedKeys.push(entry.keyStr);
      if (this.failedKeys.length > FAILED_CAP) this.failedKeys.shift();
    }
    this.o.onFailure?.(error);
    if (entry.refs === 0) this.discard(entry);
    return error;
  }

  // ---- eviction -----------------------------------------------------------------------------------------

  /** Removes an entry from the table; cancels a load in progress; frees a value. */
  private discard(entry: CacheEntry): void {
    if (entry.disposed) return;
    entry.disposed = true;
    this.cache.delete(entry);
    if (entry.state === 'loading') entry.abort.abort();
    if (entry.state === 'ready' && entry.value !== undefined) {
      try {
        entry.type.dispose?.(entry.value, entry.key);
      } catch (error) {
        this.o.log?.warn('assets', `${entry.keyStr}: dispose failed: ${String(error)}`);
      }
    }
    const deps = entry.deps;
    entry.deps = [];
    for (const d of deps) d.release();
  }

  sweep(nowMs: number = this.now()): SweepResult {
    const victims = this.cache.victims(nowMs, this.frameNo, {
      cpu: this.o.cpuBudget ?? CPU_BUDGET_BYTES,
      gpu: this.gpuBudget,
      cpuIdleMs: this.o.cpuIdleMs ?? CPU_IDLE_MS,
      gpuIdleMs: this.o.gpuIdleMs ?? GPU_IDLE_MS,
    });
    for (const e of victims) this.discard(e);
    return {
      evicted: victims.length,
      cpuBytes: this.cache.cpuBytes,
      gpuBytes: this.cache.gpuBytes,
    };
  }

  startSweeper(intervalMs = 5000): () => void {
    this.stopSweeper();
    this.sweeper = setInterval(() => {
      this.sweep();
    }, intervalMs);
    return () => {
      this.stopSweeper();
    };
  }

  private stopSweeper(): void {
    if (this.sweeper !== null) clearInterval(this.sweeper);
    this.sweeper = null;
  }

  // ---- generations and accounting -------------------------------------------------------------------------

  bumpGeneration(): void {
    this.gen += 1;
    this.o.source.setGeneration?.(this.gen);
    this.o.pool?.setGeneration?.(this.gen);
  }

  setFrame(frame: number): void {
    this.frameNo = frame;
  }

  accountGpu(handle: AssetHandle<unknown>, bytes: number): void {
    const entry = this.handleEntry.get(handle);
    if (!entry || entry.disposed) return;
    this.cache.account(entry, entry.cpuBytes, entry.gpuBytes + bytes);
    entry.uploadedFrame = this.frameNo;
  }

  dropCpu(handle: AssetHandle<unknown>): void {
    const entry = this.handleEntry.get(handle);
    if (!entry || entry.disposed) return;
    this.cache.account(entry, 0, entry.gpuBytes);
  }

  setGpuBudget(bytes: number): void {
    this.gpuBudget = bytes;
  }

  private countReupload(): number {
    let n = 0;
    for (const e of this.cache.values()) if (e.reupload === true) n += 1;
    return n;
  }

  contextLost(): number {
    let n = 0;
    for (const e of this.cache.values()) {
      if (e.disposed || e.state !== 'ready') continue;
      if (e.gpuBytes > 0 || e.reupload === true) {
        e.reupload = true;
        this.cache.account(e, e.cpuBytes, 0);
        n += 1;
      }
    }
    return n;
  }

  contextRestored(): number {
    let queued = 0;
    const uploads = this.o.uploads;
    for (const e of this.cache.values()) {
      if (e.reupload !== true || e.disposed || e.state !== 'ready') continue;
      const type = e.type;
      if (!type.upload || !uploads || e.value === undefined) {
        e.reupload = false; // the owner (41) re-creates its own GPU resources
        continue;
      }
      const job = type.upload(e.value, e.key);
      if (!job) {
        e.reupload = false;
        continue;
      }
      const entry = e;
      uploads.enqueue({
        ...job,
        run: () => {
          if (entry.disposed) return;
          job.run();
          entry.reupload = false;
          this.accountGpu(this.handleOf(entry), job.bytes);
        },
      });
      queued += 1;
    }
    return queued;
  }

  dispose(): void {
    this.stopSweeper();
    for (const e of [...this.cache.values()]) {
      e.refs = 0;
      this.discard(e);
    }
  }
}
