// Main-side decode worker pool (Plan/40 section 8, WP-CLIENT-07).
//
//   * a fixed number of module workers, each initialised with the one compiled `WebAssembly.Module`; one job at
//     a time per worker, the dispatcher hands the best queued job to the first idle worker;
//   * priorities (`Prio`, 8.3): class, then the requester's `distance`, then FIFO; `setDistance` re-ranks;
//   * transferables: input bytes are transferred to the worker (detached on this side), blobs and bitmaps are
//     transferred back, never copied;
//   * cancellation (8.4): `JobHandle.cancel()`, an `AbortSignal`, and generations (`setGeneration` cancels the
//     queued jobs of older generations and drops in-flight results of older generations);
//   * backpressure: past `maxQueuedJobs` / `maxQueuedBytes` low-priority submissions are refused with
//     `PoolBackpressureError`; `waitForCapacity()` lets a producer wait until the queue has drained;
//   * failures (8.5): a decoder error fails the job (deterministic, no retry); a trap or crash terminates and
//     respawns the worker and retries the job ONCE (on another worker when there is one), then fails it; a
//     worker that reports more than `maxWorkerMemory` bytes of wasm memory after a job is recycled;
//   * a worker that is not ready within `readyTimeoutMs` is terminated and respawned once; a second failure is
//     fatal ("cannot start decoders", recovery level L4: `onFatal`).

import type { DecodeKind, DecodeMetaOf, DecodeOptionsOf } from '../wasm/decoders';
import { Prio } from './priority';
import { PriorityQueue, type Queueable } from './queue';
import type {
  FormatsInfo,
  JobKind,
  JsonEnvelope,
  TestJobKind,
  TrapOptions,
  WorkerCaps,
  WorkerJobKind,
  WorkerRequest,
  WorkerResponse,
} from './protocol';

export { Prio } from './priority';

/** The part of `Worker` the pool needs (so tests can inject a fake). */
export interface WorkerLike {
  postMessage(message: WorkerRequest, transfer?: Transferable[]): void;
  terminate(): void;
  onmessage: ((event: { data: WorkerResponse }) => void) | null;
  onerror: ((event: { message?: string }) => void) | null;
}

export type WorkerFactory = () => WorkerLike;

/** Starts a real decode worker (bundled by Vite as a module worker). */
export const spawnDecodeWorker: WorkerFactory = () =>
  new Worker(new URL('./decode.worker.ts', import.meta.url), {
    type: 'module',
    name: 'flyff-decode',
  }) as unknown as WorkerLike;

// ---- errors -----------------------------------------------------------------------------------------------

/** The job was cancelled (handle, signal or generation). */
export class JobCancelledError extends Error {
  constructor(reason = 'cancelled') {
    super(`decode job ${reason}`);
    this.name = 'JobCancelledError';
  }
}

/** The decoder reported an error for this input (deterministic: the asset is marked failed, no retry). */
export class DecodeFailedError extends Error {
  /** `o3d`, `dds`, ... when the message names a format. */
  readonly format: string | undefined;
  /** The kind is not answered by this worker build. */
  readonly unsupported: boolean;
  constructor(message: string, format?: string, unsupported = false) {
    super(message);
    this.name = 'DecodeFailedError';
    this.format = format;
    this.unsupported = unsupported;
  }
}

/** The worker crashed or trapped twice while running the job (or the input was too large to retry). */
export class WorkerCrashError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'WorkerCrashError';
  }
}

/** The queue is full and the job's priority class is not exempt. Retry later (`waitForCapacity`). */
export class PoolBackpressureError extends Error {
  constructor(
    readonly queuedJobs: number,
    readonly queuedBytes: number,
  ) {
    super(
      `decode pool is saturated (${String(queuedJobs)} queued jobs, ${String(queuedBytes)} bytes)`,
    );
    this.name = 'PoolBackpressureError';
  }
}

/** The pool was terminated, or the decoders cannot start (recovery level L4). */
export class PoolClosedError extends Error {
  constructor(message = 'worker pool terminated') {
    super(message);
    this.name = 'PoolClosedError';
  }
}

// ---- job API ----------------------------------------------------------------------------------------------

/** Per-kind options: the decoders' options, `{ payload }` of `ping`, ... */
export interface JobOptsMap extends DecodeOptionsOf {
  readonly bitmap: undefined;
  readonly json: undefined;
  readonly chunk_check: undefined;
  readonly ping: { readonly payload: number };
  readonly 'wasm-hello': { readonly name: string };
  readonly trap: TrapOptions;
  readonly mem_grow: { readonly pages: number };
  readonly busy: { readonly ms: number };
}

/** Timing and identity of a finished job. */
export interface JobInfo {
  /** Index of the pool slot that ran it. */
  readonly worker: number;
  /** Milliseconds spent inside the worker. */
  readonly decodeMs: number;
  /** Wasm memory of that worker after the job. */
  readonly memoryBytes: number;
  /** Crashes survived before the successful run (0 or 1). */
  readonly retries: number;
}

export interface DecodeJobResult<M> {
  readonly meta: M;
  /** The blob (`byteOffset 0`, transferred, never copied) or `null` (`sfx`). */
  readonly blob: ArrayBuffer | null;
  readonly info: JobInfo;
}

export interface BitmapJobResult {
  readonly bitmap: ImageBitmap;
  readonly info: JobInfo;
}

export interface JsonJobResult extends JsonEnvelope {
  readonly info: JobInfo;
}

export interface ValueJobResult<T> {
  readonly value: T;
  readonly info: JobInfo;
}

/** What `submit` resolves to, by kind. */
export interface JobResultMap {
  readonly o3d: DecodeJobResult<DecodeMetaOf['o3d']>;
  readonly chr: DecodeJobResult<DecodeMetaOf['chr']>;
  readonly ani: DecodeJobResult<DecodeMetaOf['ani']>;
  readonly lnd: DecodeJobResult<DecodeMetaOf['lnd']>;
  readonly sfx: DecodeJobResult<DecodeMetaOf['sfx']>;
  readonly dds: DecodeJobResult<DecodeMetaOf['dds']>;
  readonly dds_bc: DecodeJobResult<DecodeMetaOf['dds_bc']>;
  readonly bitmap: BitmapJobResult;
  readonly json: JsonJobResult;
  readonly chunk_check: DecodeJobResult<unknown>;
  readonly ping: ValueJobResult<number>;
  readonly 'wasm-hello': ValueJobResult<string>;
  readonly trap: ValueJobResult<unknown>;
  readonly mem_grow: ValueJobResult<number>;
  readonly busy: ValueJobResult<null>;
}

/** What to run and how urgently. */
export interface JobSpec<K extends WorkerJobKind = WorkerJobKind> {
  readonly kind: K;
  /**
   * The input. An `ArrayBuffer` (or a `Uint8Array` covering a whole buffer) is TRANSFERRED to the worker and is
   * detached afterwards; a partial view is copied first.
   */
  readonly bytes?: ArrayBuffer | Uint8Array;
  readonly opts?: JobOptsMap[K];
  /** Default {@link Prio.Visible}. */
  readonly prio?: Prio;
  /** Tie-break inside a class: lower first (default 0); see `JobHandle.setDistance`. */
  readonly distance?: number;
  /** World generation: stale jobs are cancelled by `setGeneration` (Plan/40 8.4). */
  readonly generation?: number;
  readonly signal?: AbortSignal;
  /** Keep a copy of the input so a crash can be retried (default `true`; inputs above `retryCopyLimit` are not kept). */
  readonly retry?: boolean;
}

export type JobState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';

export interface JobHandle<K extends WorkerJobKind = WorkerJobKind> {
  readonly id: number;
  readonly kind: K;
  readonly promise: Promise<JobResultMap[K]>;
  readonly state: JobState;
  /** Cancels the job: a queued job never reaches a worker, a running job's result is discarded. */
  cancel(reason?: string): void;
  /** Re-ranks a queued job inside its class (the requester's distance changed). */
  setDistance(distance: number): void;
}

/** One dispatch to a worker (for tests and the dev page). */
export interface DispatchEvent {
  readonly id: number;
  readonly kind: WorkerJobKind;
  readonly prio: Prio;
  readonly distance: number;
  readonly worker: number;
  /** 0 for the first run, 1 for the retry after a crash. */
  readonly attempt: number;
}

/** Something notable that happened to a worker (logging). */
export type PoolEvent =
  | { readonly type: 'ready'; readonly worker: number; readonly wasmVersion: string }
  | {
      readonly type: 'respawn';
      readonly worker: number;
      readonly reason: 'crash' | 'trap' | 'memory' | 'timeout' | 'init';
    }
  | { readonly type: 'retry'; readonly job: number; readonly kind: WorkerJobKind }
  | {
      readonly type: 'failed';
      readonly job: number;
      readonly kind: WorkerJobKind;
      readonly error: string;
    };

export interface PoolOptions {
  /** Enables the test-only job kinds (`trap`, `mem_grow`, `busy`) in the workers. Never set in production. */
  readonly testHooks?: boolean;
  /**
   * `false` when the GPU lacks `WEBGL_compressed_texture_s3tc` (`caps.s3tc`, Plan/40 3.2): `dds` jobs are then run
   * as `dds_bc` (RGBA8 levels; the result's `meta.pixel` says so). Default `true`.
   */
  readonly s3tc?: boolean;
  /** Recycle a worker whose wasm memory exceeds this after a job (default 64 MiB, Plan/01 12.2). */
  readonly maxWorkerMemory?: number;
  /** A worker that is not ready in this time is respawned once (default 10 s). */
  readonly readyTimeoutMs?: number;
  /** Queue limits for low-priority jobs (defaults 2048 jobs, 256 MiB). */
  readonly maxQueuedJobs?: number;
  readonly maxQueuedBytes?: number;
  /** Largest input kept for the crash retry (default 16 MiB). */
  readonly retryCopyLimit?: number;
  /** Classes at or above this value are refused when the queue is full (default {@link Prio.Statics}). */
  readonly backpressureFrom?: Prio;
  readonly onDispatch?: (event: DispatchEvent) => void;
  readonly onEvent?: (event: PoolEvent) => void;
  /** The decoders cannot start (recovery level L4). */
  readonly onFatal?: (error: Error) => void;
}

export interface PoolStats {
  readonly size: number;
  readonly idle: number;
  readonly busy: number;
  readonly starting: number;
  readonly queued: number;
  readonly queuedBytes: number;
  readonly dispatched: number;
  readonly completed: number;
  readonly failed: number;
  readonly cancelled: number;
  readonly crashes: number;
  readonly respawns: number;
  readonly recycled: number;
  readonly maxQueued: number;
}

export interface PoolPressure {
  readonly queued: number;
  readonly queuedBytes: number;
  readonly running: number;
  /** `true` when low-priority submissions would be refused. */
  readonly saturated: boolean;
}

/** `clamp(navigator.hardwareConcurrency - 2, 2, 4)` (Plan/40 8.1). */
export function defaultPoolSize(): number {
  const cores = typeof navigator === 'object' ? navigator.hardwareConcurrency : 4;
  return Math.min(4, Math.max(2, (Number.isFinite(cores) ? cores : 4) - 2));
}

// ---- internals --------------------------------------------------------------------------------------------

interface JobRecord extends Queueable {
  readonly id: number;
  readonly kind: WorkerJobKind;
  readonly opts: unknown;
  readonly seq: number;
  readonly generation: number | null;
  readonly prio: Prio;
  distance: number;
  /** The caller's buffer on its way to the worker (null once transferred). */
  payload: ArrayBuffer | null;
  /** Private copy kept for the crash retry. */
  retained: ArrayBuffer | null;
  /** A crash may be retried: the job has no input, or its input was kept. */
  retryable: boolean;
  readonly byteLength: number;
  attempts: number;
  state: JobState;
  slot: Slot | null;
  /** Slot that crashed on this job: another worker is preferred for the retry. */
  avoid: number;
  resolve(value: JobResultMap[WorkerJobKind]): void;
  reject(error: Error): void;
  signal: AbortSignal | undefined;
  onAbort: (() => void) | undefined;
}

type SlotState = 'starting' | 'idle' | 'busy' | 'dead';

interface Slot {
  readonly index: number;
  state: SlotState;
  worker: WorkerLike | null;
  /** Incarnation counter: messages of terminated workers are ignored. */
  generation: number;
  job: JobRecord | null;
  /** Start attempts of the current incarnation chain (reset on `ready`). */
  startAttempts: number;
  timer: ReturnType<typeof setTimeout> | null;
  /** Resolves on the first `ready`, rejects when the slot cannot start. */
  ready: Promise<string>;
  resolveReady: (version: string) => void;
  rejectReady: (error: Error) => void;
  readySettled: boolean;
}

const MIB = 1024 * 1024;

/** The pool. Create it with {@link WorkerPool.create}. */
export class WorkerPool {
  private readonly slots: Slot[] = [];
  private readonly queue = new PriorityQueue<JobRecord>();
  private readonly jobs = new Map<number, JobRecord>();
  private readonly waiters: (() => void)[] = [];
  private nextId = 1;
  private nextSeq = 0;
  private queuedJobs = 0;
  private queuedBytesTotal = 0;
  private generationFloor = 0;
  private closed = false;
  private fatal: Error | null = null;
  private wasmVersionValue = '';
  private infoValue: FormatsInfo | null = null;
  private counters = {
    dispatched: 0,
    completed: 0,
    failed: 0,
    cancelled: 0,
    crashes: 0,
    respawns: 0,
    recycled: 0,
    maxQueued: 0,
  };

  private readonly maxWorkerMemory: number;
  private readonly readyTimeoutMs: number;
  private readonly maxQueuedJobs: number;
  private readonly maxQueuedBytes: number;
  private readonly retryCopyLimit: number;
  private readonly backpressureFrom: Prio;
  private readonly caps: WorkerCaps;

  private constructor(
    private readonly module: WebAssembly.Module,
    private readonly factory: WorkerFactory,
    private readonly options: PoolOptions,
  ) {
    this.maxWorkerMemory = options.maxWorkerMemory ?? 64 * MIB;
    this.readyTimeoutMs = options.readyTimeoutMs ?? 10_000;
    this.maxQueuedJobs = options.maxQueuedJobs ?? 2048;
    this.maxQueuedBytes = options.maxQueuedBytes ?? 256 * MIB;
    this.retryCopyLimit = options.retryCopyLimit ?? 16 * MIB;
    this.backpressureFrom = options.backpressureFrom ?? Prio.Statics;
    this.caps = { testHooks: options.testHooks === true };
  }

  /**
   * Starts `size` workers and initialises each with the compiled wasm module. Resolves when every worker
   * answered `ready`; rejects (and calls `onFatal`) when one cannot start even after its respawn.
   */
  static async create(
    size: number,
    module: WebAssembly.Module,
    factory: WorkerFactory = spawnDecodeWorker,
    options: PoolOptions = {},
  ): Promise<{ pool: WorkerPool; wasmVersion: string }> {
    const pool = new WorkerPool(module, factory, options);
    const count = Math.max(1, Math.floor(size));
    for (let i = 0; i < count; i += 1) pool.slots.push(pool.newSlot(i));
    for (const slot of pool.slots) pool.startWorker(slot);
    try {
      await Promise.all(pool.slots.map((s) => s.ready));
    } catch (e) {
      pool.terminate();
      throw e;
    }
    return { pool, wasmVersion: pool.wasmVersionValue };
  }

  // ---- slots ----------------------------------------------------------------------------------------------

  private newSlot(index: number): Slot {
    let resolveReady: (v: string) => void = () => undefined;
    let rejectReady: (e: Error) => void = () => undefined;
    const ready = new Promise<string>((resolve, reject) => {
      resolveReady = resolve;
      rejectReady = reject;
    });
    // an unhandled rejection of a slot that nobody awaits (respawn after create) must not surface
    ready.catch(() => undefined);
    return {
      index,
      state: 'dead',
      worker: null,
      generation: 0,
      job: null,
      startAttempts: 0,
      timer: null,
      ready,
      resolveReady,
      rejectReady,
      readySettled: false,
    };
  }

  /** (Re)starts the worker of `slot`. */
  private startWorker(slot: Slot): void {
    if (this.closed) return;
    slot.generation += 1;
    const incarnation = slot.generation;
    slot.state = 'starting';
    slot.startAttempts += 1;
    const worker = this.factory();
    slot.worker = worker;
    worker.onmessage = (event) => {
      this.onMessage(slot, incarnation, event.data);
    };
    worker.onerror = (event) => {
      this.onWorkerError(slot, incarnation, event.message ?? 'worker error');
    };
    slot.timer = setTimeout(() => {
      this.onStartFailure(slot, incarnation, 'timeout');
    }, this.readyTimeoutMs);
    worker.postMessage({ type: 'init', module: this.module, caps: this.caps });
  }

  private clearTimer(slot: Slot): void {
    if (slot.timer !== null) clearTimeout(slot.timer);
    slot.timer = null;
  }

  /** Start failed (timeout, `fatal`, error before `ready`): retry once, then give up (L4). */
  private onStartFailure(slot: Slot, incarnation: number, reason: 'timeout' | 'init'): void {
    if (slot.generation !== incarnation || slot.state !== 'starting') return;
    this.clearTimer(slot);
    slot.worker?.terminate();
    slot.worker = null;
    if (slot.startAttempts < 2) {
      this.counters.respawns += 1;
      this.options.onEvent?.({ type: 'respawn', worker: slot.index, reason });
      this.startWorker(slot);
      return;
    }
    slot.state = 'dead';
    const error = new PoolClosedError(
      `cannot start decoders (worker ${String(slot.index)}: ${reason})`,
    );
    if (!slot.readySettled) {
      slot.readySettled = true;
      slot.rejectReady(error);
    }
    this.failFatally(error);
  }

  private failFatally(error: Error): void {
    if (this.fatal) return;
    this.fatal = error;
    this.options.onFatal?.(error);
    this.failAll(error);
  }

  private onMessage(slot: Slot, incarnation: number, message: WorkerResponse): void {
    if (slot.generation !== incarnation) return; // a terminated incarnation
    switch (message.type) {
      case 'ready':
        this.clearTimer(slot);
        slot.state = 'idle';
        slot.startAttempts = 0;
        this.wasmVersionValue = message.wasmVersion;
        this.infoValue = message.info;
        if (!slot.readySettled) {
          slot.readySettled = true;
          slot.resolveReady(message.wasmVersion);
        }
        this.options.onEvent?.({
          type: 'ready',
          worker: slot.index,
          wasmVersion: message.wasmVersion,
        });
        this.pump();
        return;
      case 'fatal':
        this.onStartFailure(slot, incarnation, 'init');
        return;
      case 'result':
        this.onResult(slot, message);
        return;
    }
  }

  private onWorkerError(slot: Slot, incarnation: number, message: string): void {
    if (slot.generation !== incarnation) return;
    if (slot.state === 'starting') {
      this.onStartFailure(slot, incarnation, 'init');
      return;
    }
    this.handleCrash(slot, 'crash', message);
  }

  // ---- submission -----------------------------------------------------------------------------------------

  /** Queues a job; the returned handle carries the promise, `cancel` and `setDistance`. */
  submit<K extends WorkerJobKind>(spec: JobSpec<K>): JobHandle<K> {
    const id = this.nextId++;
    let resolve: (v: JobResultMap[K]) => void = () => undefined;
    let reject: (e: Error) => void = () => undefined;
    const promise = new Promise<JobResultMap[K]>((res, rej) => {
      resolve = res;
      reject = rej;
    });
    // a caller that drops the handle (a cancelled prefetch) must not cause an unhandled rejection; callers that
    // await the promise still see every failure
    promise.catch(() => undefined);
    const prio = spec.prio ?? Prio.Visible;
    const kind = (spec.kind === 'dds' && this.options.s3tc === false ? 'dds_bc' : spec.kind) as K;
    const { payload, length } = this.takeInput(spec);
    const record: JobRecord = {
      id,
      kind,
      opts: spec.opts,
      seq: this.nextSeq++,
      generation: spec.generation ?? null,
      prio,
      distance: spec.distance ?? 0,
      payload,
      retained: null,
      retryable: false,
      byteLength: length,
      attempts: 0,
      state: 'queued',
      slot: null,
      avoid: -1,
      version: 0,
      queued: false,
      resolve,
      reject,
      signal: spec.signal,
      onAbort: undefined,
    };
    const handle: JobHandle<K> = {
      id,
      kind,
      promise,
      get state() {
        return record.state;
      },
      cancel: (reason) => {
        this.cancelJob(record, reason);
      },
      setDistance: (distance) => {
        this.rerank(record, distance);
      },
    };

    // refuse before anything else is done with the input
    const refusal = this.admit(record, spec.retry !== false);
    if (refusal) {
      record.state = 'failed';
      this.counters.failed += 1;
      reject(refusal);
      return handle;
    }
    if (record.generation !== null && record.generation < this.generationFloor) {
      record.state = 'cancelled';
      this.counters.cancelled += 1;
      reject(new JobCancelledError('belongs to an older generation'));
      return handle;
    }
    if (spec.signal?.aborted) {
      record.state = 'cancelled';
      this.counters.cancelled += 1;
      reject(new JobCancelledError('aborted'));
      return handle;
    }
    if (spec.signal) {
      record.onAbort = () => {
        this.cancelJob(record, 'aborted');
      };
      spec.signal.addEventListener('abort', record.onAbort, { once: true });
    }
    this.jobs.set(id, record);
    record.queued = true;
    this.queuedJobs += 1;
    this.queuedBytesTotal += record.byteLength;
    this.counters.maxQueued = Math.max(this.counters.maxQueued, this.queuedJobs);
    this.queue.push(record, record);
    this.pump();
    return handle;
  }

  /** `submit(spec).promise`. */
  run<K extends WorkerJobKind>(spec: JobSpec<K>): Promise<JobResultMap[K]> {
    return this.submit(spec).promise;
  }

  /** The input as a transferable buffer; a partial view is copied. */
  private takeInput(spec: JobSpec): { payload: ArrayBuffer | null; length: number } {
    const bytes = spec.bytes;
    if (bytes === undefined) return { payload: null, length: 0 };
    if (bytes instanceof ArrayBuffer) return { payload: bytes, length: bytes.byteLength };
    if (
      bytes.byteOffset === 0 &&
      bytes.byteLength === bytes.buffer.byteLength &&
      bytes.buffer instanceof ArrayBuffer
    ) {
      return { payload: bytes.buffer, length: bytes.byteLength };
    }
    return { payload: bytes.slice().buffer, length: bytes.byteLength };
  }

  /** Backpressure and kind checks; returns the error that refuses the job, if any. */
  private admit(record: JobRecord, keepForRetry: boolean): Error | null {
    if (this.closed) return this.fatal ?? new PoolClosedError();
    if (this.fatal) return this.fatal;
    if (
      record.prio >= this.backpressureFrom &&
      (this.queuedJobs >= this.maxQueuedJobs ||
        this.queuedBytesTotal + record.byteLength > this.maxQueuedBytes)
    ) {
      return new PoolBackpressureError(this.queuedJobs, this.queuedBytesTotal);
    }
    if (keepForRetry) {
      if (record.payload === null) {
        record.retryable = true; // nothing to keep
      } else if (record.byteLength <= this.retryCopyLimit) {
        record.retained = record.payload.slice(0);
        record.retryable = true;
      }
    }
    return null;
  }

  // ---- cancellation and ranking ---------------------------------------------------------------------------

  private cancelJob(record: JobRecord, reason = 'cancelled'): void {
    if (record.state === 'queued') {
      this.dequeue(record);
      this.finish(record, 'cancelled');
      this.counters.cancelled += 1;
      record.reject(new JobCancelledError(reason));
      this.pump();
    } else if (record.state === 'running') {
      // decoders are synchronous and cannot be interrupted: the result is discarded on arrival
      this.finish(record, 'cancelled');
      this.counters.cancelled += 1;
      record.reject(new JobCancelledError(reason));
    }
  }

  private rerank(record: JobRecord, distance: number): void {
    if (record.state !== 'queued' || record.distance === distance) return;
    record.distance = distance;
    record.version += 1;
    this.queue.push(record, record);
    if (this.queue.entries > 64 + 4 * this.queuedJobs) this.queue.compact();
  }

  /**
   * Cancels every queued job of an older generation and drops the results of in-flight ones (Plan/40 8.4).
   * Later submissions of an older generation are refused.
   */
  setGeneration(generation: number): void {
    if (generation <= this.generationFloor) return;
    this.generationFloor = generation;
    for (const record of [...this.jobs.values()]) {
      if (record.generation !== null && record.generation < generation) {
        this.cancelJob(record, 'belongs to an older generation');
      }
    }
  }

  private dequeue(record: JobRecord): void {
    if (!record.queued) return;
    record.queued = false;
    record.version += 1;
    this.queuedJobs -= 1;
    this.queuedBytesTotal -= record.byteLength;
    if (this.queue.entries > 64 + 4 * this.queuedJobs) this.queue.compact(); // stale entries dominate
    this.releaseWaiters();
  }

  private finish(record: JobRecord, state: JobState): void {
    record.state = state;
    record.payload = null;
    record.retained = null;
    if (record.signal && record.onAbort) record.signal.removeEventListener('abort', record.onAbort);
    this.jobs.delete(record.id);
  }

  // ---- dispatch -------------------------------------------------------------------------------------------

  private pump(): void {
    if (this.closed || this.fatal) return;
    for (const slot of this.slots) {
      if (slot.state !== 'idle') continue;
      // a job that crashed a worker prefers another one (a single-worker pool has no choice)
      const only = this.slots.length === 1;
      const job = this.queue.pop((j) => only || j.avoid !== slot.index);
      if (!job) continue;
      this.dispatch(slot, job);
    }
  }

  private dispatch(slot: Slot, job: JobRecord): void {
    this.dequeue(job);
    const worker = slot.worker;
    if (!worker) {
      this.finish(job, 'failed');
      job.reject(new WorkerCrashError('no worker available'));
      return;
    }
    job.state = 'running';
    job.slot = slot;
    slot.state = 'busy';
    slot.job = job;
    this.counters.dispatched += 1;
    this.options.onDispatch?.({
      id: job.id,
      kind: job.kind,
      prio: job.prio,
      distance: job.distance,
      worker: slot.index,
      attempt: job.attempts,
    });
    // the first run transfers the caller's buffer, a retry transfers the private copy
    const bytes = job.attempts === 0 ? job.payload : job.retained;
    job.payload = null;
    const request: WorkerRequest = {
      type: 'job',
      id: job.id,
      kind: job.kind,
      bytes,
      ...(job.opts === undefined ? {} : { opts: job.opts }),
    };
    try {
      if (bytes) {
        worker.postMessage(request, [bytes]);
      } else {
        worker.postMessage(request);
      }
    } catch (e) {
      // the request could not be cloned (e.g. a function inside `opts`): the job fails, the worker is fine
      slot.state = 'idle';
      slot.job = null;
      this.finish(job, 'failed');
      this.counters.failed += 1;
      job.reject(new DecodeFailedError(e instanceof Error ? e.message : String(e)));
      queueMicrotask(() => {
        this.pump();
      });
    }
    // after the first transfer the pool's own copy (if any) stays for a possible retry
  }

  // ---- results --------------------------------------------------------------------------------------------

  private onResult(slot: Slot, message: Extract<WorkerResponse, { type: 'result' }>): void {
    const job = slot.job;
    if (job?.id !== message.id) return; // not what this slot is running (discarded earlier)
    if (!message.ok && message.trap === true) {
      // the instance is dead (whatever happened to the job): respawn, and retry a live job once
      this.handleCrash(slot, 'trap', message.error);
      return;
    }
    slot.job = null;
    slot.state = 'idle';

    if (job.state === 'running') {
      if (message.ok) {
        const info: JobInfo = {
          worker: slot.index,
          decodeMs: message.ms,
          memoryBytes: message.mem,
          retries: job.attempts,
        };
        this.finish(job, 'done');
        this.counters.completed += 1;
        job.resolve(this.shape(job, message, info));
      } else {
        this.finish(job, 'failed');
        this.counters.failed += 1;
        this.options.onEvent?.({
          type: 'failed',
          job: job.id,
          kind: job.kind,
          error: message.error,
        });
        job.reject(
          new DecodeFailedError(message.error, message.format, message.unsupported === true),
        );
      }
    }
    // a cancelled job: the result is dropped (and a bitmap is released)
    if (message.ok && job.state === 'cancelled') message.bitmap?.close();

    if (message.mem > this.maxWorkerMemory) {
      this.counters.recycled += 1;
      this.recycle(slot, 'memory');
    }
    this.pump();
  }

  /** The typed result of a job from the worker's answer (the kind decides the shape). */
  private shape(
    job: JobRecord,
    message: Extract<WorkerResponse, { type: 'result'; ok: true }>,
    info: JobInfo,
  ): JobResultMap[WorkerJobKind] {
    const kind = job.kind;
    if (kind === 'bitmap') {
      if (!message.bitmap) throw new DecodeFailedError('bitmap: the worker returned no bitmap');
      return { bitmap: message.bitmap, info };
    }
    if (kind === 'json') {
      const envelope = message.meta as JsonEnvelope;
      return { bundle: envelope.bundle, schema: envelope.schema, data: envelope.data, info };
    }
    if (kind === 'ping' || kind === 'wasm-hello' || isTestKind(kind)) {
      return { value: message.meta, info };
    }
    return { meta: message.meta, blob: message.blob, info };
  }

  // ---- failures -------------------------------------------------------------------------------------------

  /** A worker died (error event or trap): terminate, respawn, retry the job once. */
  private handleCrash(slot: Slot, reason: 'crash' | 'trap', error: string): void {
    this.counters.crashes += 1;
    const job = slot.job;
    slot.job = null;
    slot.worker?.terminate();
    slot.worker = null;
    slot.state = 'dead';
    slot.startAttempts = 0;
    this.counters.respawns += 1;
    this.options.onEvent?.({ type: 'respawn', worker: slot.index, reason });
    this.startWorker(slot);

    if (job?.state === 'running') {
      job.attempts += 1;
      job.slot = null;
      if (job.attempts <= 1 && job.retryable) {
        this.options.onEvent?.({ type: 'retry', job: job.id, kind: job.kind });
        job.state = 'queued';
        job.avoid = slot.index;
        job.version += 1;
        job.queued = true;
        this.queuedJobs += 1;
        this.queuedBytesTotal += job.byteLength;
        this.queue.push(job, job); // keeps its seq: first in line of its class
      } else {
        const why = !job.retryable && job.attempts <= 1 ? ' (input not kept for a retry)' : '';
        this.finish(job, 'failed');
        this.counters.failed += 1;
        this.options.onEvent?.({ type: 'failed', job: job.id, kind: job.kind, error });
        job.reject(
          new WorkerCrashError(
            `decode worker ${reason === 'trap' ? 'trapped' : 'crashed'}: ${error}${why}`,
          ),
        );
      }
    }
    this.pump();
  }

  /** Terminates and respawns a healthy worker (memory recycling). */
  private recycle(slot: Slot, reason: 'memory'): void {
    slot.worker?.terminate();
    slot.worker = null;
    slot.state = 'dead';
    slot.startAttempts = 0;
    this.counters.respawns += 1;
    this.options.onEvent?.({ type: 'respawn', worker: slot.index, reason });
    this.startWorker(slot);
  }

  private failAll(error: Error): void {
    for (const record of [...this.jobs.values()]) {
      if (record.state === 'queued') this.dequeue(record);
      this.finish(record, 'failed');
      this.counters.failed += 1;
      record.reject(error);
    }
  }

  // ---- backpressure ---------------------------------------------------------------------------------------

  /** `true` when low-priority submissions would be refused right now. */
  get saturated(): boolean {
    return this.queuedJobs >= this.maxQueuedJobs || this.queuedBytesTotal >= this.maxQueuedBytes;
  }

  pressure(): PoolPressure {
    return {
      queued: this.queuedJobs,
      queuedBytes: this.queuedBytesTotal,
      running: this.slots.filter((s) => s.state === 'busy').length,
      saturated: this.saturated,
    };
  }

  /** Resolves when the queue has drained below half of its limits (a producer's pause point). */
  waitForCapacity(): Promise<void> {
    if (!this.shouldWait()) return Promise.resolve();
    return new Promise((resolve) => {
      this.waiters.push(resolve);
    });
  }

  private shouldWait(): boolean {
    return (
      this.queuedJobs * 2 >= this.maxQueuedJobs || this.queuedBytesTotal * 2 >= this.maxQueuedBytes
    );
  }

  private releaseWaiters(): void {
    if (this.waiters.length === 0 || this.shouldWait()) return;
    const waiting = this.waiters.splice(0);
    for (const resolve of waiting) resolve();
  }

  // ---- introspection --------------------------------------------------------------------------------------

  /** Number of workers. */
  get size(): number {
    return this.slots.length;
  }

  /** Wasm version reported by the workers. */
  get wasmVersion(): string {
    return this.wasmVersionValue;
  }

  /** `formats_info()` as reported by a worker at start-up. */
  get formats(): FormatsInfo | null {
    return this.infoValue;
  }

  stats(): PoolStats {
    const count = (s: SlotState): number => this.slots.filter((x) => x.state === s).length;
    return {
      size: this.slots.length,
      idle: count('idle'),
      busy: count('busy'),
      starting: count('starting'),
      queued: this.queuedJobs,
      queuedBytes: this.queuedBytesTotal,
      ...this.counters,
    };
  }

  // ---- convenience ----------------------------------------------------------------------------------------

  /** Round-trips a number through a worker (liveness check). */
  async ping(payload: number): Promise<number> {
    return (await this.run({ kind: 'ping', opts: { payload }, prio: Prio.Critical })).value;
  }

  /** Calls the `hello` export of the wasm module inside a worker. */
  async wasmHello(name: string): Promise<string> {
    return (await this.run({ kind: 'wasm-hello', opts: { name }, prio: Prio.Critical })).value;
  }

  // ---- shutdown -------------------------------------------------------------------------------------------

  /** Stops every worker and rejects every outstanding job. */
  terminate(): void {
    if (this.closed) return;
    this.closed = true;
    const error = new PoolClosedError();
    for (const slot of this.slots) {
      this.clearTimer(slot);
      slot.generation += 1;
      slot.worker?.terminate();
      slot.worker = null;
      slot.state = 'dead';
      slot.job = null;
      if (!slot.readySettled) {
        slot.readySettled = true;
        slot.rejectReady(error);
      }
    }
    this.failAll(error);
    for (const resolve of this.waiters.splice(0)) resolve();
    this.slots.length = 0;
  }
}

function isTestKind(kind: WorkerJobKind): kind is TestJobKind {
  return kind === 'trap' || kind === 'mem_grow' || kind === 'busy';
}

/** The kinds `submit` accepts that map to decode exports (for the dev page and tests). */
export type { DecodeKind, JobKind };
