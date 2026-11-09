// The fetch scheduler (Plan/40 section 9.2, WP-CLIENT-09): every asset request of the page goes through it.
//
// * concurrency 6 on `http:` (the browser's HTTP/1.1 per-host limit), 12 on `https:` (h2); the WebSocket has its own
//   connection and does not count;
// * a priority queue of the eight classes of Plan/40 section 8.3 (`Prio`), lower `distance` first inside a class, then
//   FIFO; the same order the worker dispatcher uses;
// * a request for a URL key that is already queued, running or waiting joins it (one network request), with the best
//   priority of its joiners; every joiner has its own `AbortSignal`, the request is cancelled when the last one left;
// * `fetch(url, { cache: 'default' })`: immutable URLs make the HTTP cache the persistent cache (CA11);
// * retries: 503/429 with `Retry-After` wait that long (+ up to 500 ms jitter), at most 10 times, and report the
//   request as "waiting for preparation" (Plan/40 section 9.9); a network error retries after 0.5, 1 and 2 s; other
//   5xx retry once after 1 s; 404 fails without a retry; 401 fails as well (a gated asset without the cookie);
// * generations: `setGeneration(g)` cancels the QUEUED requests of older world generations (Plan/40 section 8.4).

import { PriorityQueue, type Queueable } from '../workers/queue';
import type { Prio } from '../workers/priority';
import { AssetError } from './errors';

/** The browser's connection limits per host (Plan/40 section 9.2). */
export const HTTP_CONCURRENCY = 6;
export const HTTPS_CONCURRENCY = 12;
/** `Retry-After` waits per request (Plan/40 section 9.2). */
export const MAX_RETRY_AFTER_TRIES = 10;
/** Network-error retry delays in ms (Plan/40 section 9.2). */
export const NETWORK_RETRY_MS: readonly number[] = [500, 1000, 2000];
/** The single retry of a plain 5xx. */
export const SERVER_RETRY_MS = 1000;
/** Upper bound of the random delay added to a `Retry-After` wait. */
export const RETRY_JITTER_MS = 500;

/** The browser concurrency limit for a page protocol (`location.protocol`). */
export function concurrencyFor(protocol: string): number {
  return protocol === 'https:' ? HTTPS_CONCURRENCY : HTTP_CONCURRENCY;
}

export interface FetchRequest {
  /** The de-duplication key (`keyString` of the asset key, or the URL for ad-hoc requests). */
  readonly key: string;
  readonly url: string;
  readonly prio: Prio;
  /** Tie-break inside a class: lower first (default 0). */
  readonly distance?: number;
  /** World generation; `null`/absent = never cancelled by a generation bump. */
  readonly generation?: number | null;
  readonly signal?: AbortSignal;
}

export interface FetchSchedulerOptions {
  /** `fetch` (default: the global one, resolved at call time). */
  readonly fetch?: typeof fetch;
  /** In-flight limit (default: from `location.protocol`). */
  readonly concurrency?: number;
  readonly timers?: {
    setTimeout(cb: () => void, ms: number): unknown;
    clearTimeout(id: unknown): void;
  };
  /** `[0, 1)` source of the `Retry-After` jitter (default `Math.random`). */
  readonly random?: () => number;
  /** A request started or stopped waiting for the server to build the asset (503/429 with `Retry-After`). */
  readonly onWaiting?: (key: string, waiting: boolean) => void;
  /** A request was created (`true`) or settled, failed or dropped (`false`): the preparing-assets tracker times them. */
  readonly onPending?: (key: string, pending: boolean) => void;
  readonly log?: { warn(scope: string, message: string): void };
}

export interface FetchSchedulerStats {
  /** Requests on the wire right now. */
  readonly inflight: number;
  readonly queued: number;
  /** Sleeping between retries (not counted against the concurrency). */
  readonly waiting: number;
  readonly completed: number;
  readonly failed: number;
  readonly retries: number;
  readonly cancelled: number;
  /** The highest `inflight` ever observed. */
  readonly maxInflight: number;
  readonly bytes: number;
}

interface Joiner {
  resolve(buf: ArrayBuffer): void;
  reject(error: Error): void;
  signal: AbortSignal | undefined;
  onAbort: (() => void) | undefined;
}

type EntryState = 'queued' | 'running' | 'waiting' | 'done';

interface Entry extends Queueable {
  readonly key: string;
  readonly url: string;
  prio: Prio;
  distance: number;
  seq: number;
  generation: number | null;
  state: EntryState;
  joiners: Joiner[];
  controller: AbortController | null;
  timer: unknown;
  networkTries: number;
  serverTries: number;
  retryAfterTries: number;
  /** `true` after the first 503 (the request waits for a build). */
  preparing: boolean;
}

/** `Retry-After` in whole seconds (HTTP-date forms and garbage count as 2 s, the server's own value). */
export function parseRetryAfter(value: string | null): number | null {
  if (value === null) return null;
  const s = Number(value.trim());
  return Number.isFinite(s) && s >= 0 ? s : 2;
}

export class FetchScheduler {
  private readonly queue = new PriorityQueue<Entry>();
  private readonly entries = new Map<string, Entry>();
  private readonly limit: number;
  private seq = 0;
  private running = 0;
  private counters = {
    completed: 0,
    failed: 0,
    retries: 0,
    cancelled: 0,
    maxInflight: 0,
    bytes: 0,
  };

  constructor(private readonly options: FetchSchedulerOptions = {}) {
    this.limit = Math.max(
      1,
      options.concurrency ??
        concurrencyFor(typeof location === 'undefined' ? 'http:' : location.protocol),
    );
  }

  /** The in-flight limit. */
  get concurrency(): number {
    return this.limit;
  }

  get stats(): FetchSchedulerStats {
    let waiting = 0;
    let queued = 0;
    for (const e of this.entries.values()) {
      if (e.state === 'waiting') waiting += 1;
      else if (e.state === 'queued') queued += 1;
    }
    return { inflight: this.running, queued, waiting, ...this.counters };
  }

  /** Keys of the requests that are queued, running or waiting. */
  pendingKeys(): string[] {
    return [...this.entries.keys()];
  }

  /**
   * Fetches `req.url`. The promise resolves with the body, rejects with an `AssetError`. A second request for
   * the same key joins the first (its buffer is a copy, so a joiner may transfer its own).
   */
  fetch(req: FetchRequest): Promise<ArrayBuffer> {
    return new Promise<ArrayBuffer>((resolve, reject) => {
      if (req.signal?.aborted === true) {
        reject(new AssetError('aborted', req.key, 'aborted before the request started'));
        return;
      }
      const joiner: Joiner = { resolve, reject, signal: req.signal, onAbort: undefined };
      let entry = this.entries.get(req.key);
      if (entry === undefined) {
        entry = {
          key: req.key,
          url: req.url,
          prio: req.prio,
          distance: req.distance ?? 0,
          seq: this.seq++,
          generation: req.generation ?? null,
          state: 'queued',
          joiners: [],
          controller: null,
          timer: null,
          networkTries: 0,
          serverTries: 0,
          retryAfterTries: 0,
          preparing: false,
          version: 0,
          queued: true,
        };
        this.entries.set(req.key, entry);
        this.options.onPending?.(req.key, true);
        this.attach(entry, joiner);
        this.enqueue(entry);
        this.pump();
        return;
      }
      this.attach(entry, joiner);
      this.merge(entry, req);
    });
  }

  /** Re-ranks a queued request inside its class (the requester's distance changed). */
  setDistance(key: string, distance: number): void {
    const e = this.entries.get(key);
    if (!e || e.distance === distance) return;
    e.distance = distance;
    if (e.state === 'queued') this.enqueue(e);
  }

  /** Cancels the queued and waiting requests of generations older than `generation` (Plan/40 section 8.4). */
  setGeneration(generation: number): void {
    for (const e of [...this.entries.values()]) {
      if (e.generation !== null && e.generation < generation && e.state !== 'running') {
        this.fail(e, new AssetError('aborted', e.key, `stale generation ${String(e.generation)}`));
      }
    }
  }

  /** Cancels every request (shutdown, tests). */
  cancelAll(reason = 'cancelled'): void {
    for (const e of [...this.entries.values()])
      this.fail(e, new AssetError('aborted', e.key, reason));
  }

  // ---- joining and ranking ----------------------------------------------------------------------------------

  private attach(entry: Entry, joiner: Joiner): void {
    entry.joiners.push(joiner);
    const { signal } = joiner;
    if (signal) {
      joiner.onAbort = () => {
        this.leave(entry, joiner);
      };
      signal.addEventListener('abort', joiner.onAbort, { once: true });
    }
  }

  /** A joiner with a better rank or a longer-lived generation upgrades the shared request. */
  private merge(entry: Entry, req: FetchRequest): void {
    let changed = false;
    if (req.prio < entry.prio) {
      entry.prio = req.prio;
      changed = true;
    }
    const d = req.distance ?? 0;
    if (req.prio <= entry.prio && d < entry.distance) {
      entry.distance = d;
      changed = true;
    }
    if (req.generation === null || req.generation === undefined) entry.generation = null;
    else if (entry.generation !== null && req.generation > entry.generation) {
      entry.generation = req.generation;
    }
    if (changed && entry.state === 'queued') this.enqueue(entry);
  }

  private enqueue(entry: Entry): void {
    entry.version += 1;
    entry.queued = true;
    this.queue.push(entry, { prio: entry.prio, distance: entry.distance, seq: entry.seq });
    if (this.queue.entries > 64 + 4 * this.entries.size) this.queue.compact();
  }

  private leave(entry: Entry, joiner: Joiner): void {
    const at = entry.joiners.indexOf(joiner);
    if (at < 0) return;
    entry.joiners.splice(at, 1);
    this.counters.cancelled += 1;
    joiner.reject(new AssetError('aborted', entry.key, 'aborted by the requester'));
    if (entry.joiners.length === 0) this.drop(entry);
  }

  /** Removes an entry nobody waits for: queued -> never started, running -> aborted, waiting -> timer cleared. */
  private drop(entry: Entry): void {
    if (entry.state === 'done') return;
    entry.queued = false;
    entry.version += 1;
    entry.state = 'done';
    if (entry.timer !== null) this.timers().clearTimeout(entry.timer);
    entry.timer = null;
    if (entry.controller) entry.controller.abort();
    this.entries.delete(entry.key);
    this.options.onPending?.(entry.key, false);
    if (entry.preparing) this.options.onWaiting?.(entry.key, false);
    // a running request's slot is released when its aborted fetch settles (see `run`)
    this.pump();
  }

  // ---- running ----------------------------------------------------------------------------------------------

  private pump(): void {
    while (this.running < this.limit) {
      const e = this.queue.pop();
      if (!e) return;
      this.start(e);
    }
  }

  private timers(): NonNullable<FetchSchedulerOptions['timers']> {
    return (
      this.options.timers ?? {
        setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
        clearTimeout: (id) => {
          globalThis.clearTimeout(id as number);
        },
      }
    );
  }

  private start(entry: Entry): void {
    entry.queued = false;
    entry.state = 'running';
    entry.controller = new AbortController();
    this.running += 1;
    this.counters.maxInflight = Math.max(this.counters.maxInflight, this.running);
    void this.run(entry, entry.controller);
  }

  private async run(entry: Entry, controller: AbortController): Promise<void> {
    let outcome: Outcome;
    try {
      outcome = await this.attempt(entry, controller);
    } catch (error) {
      outcome = { kind: 'failed', error: toAssetError(error, entry.key) };
    }
    this.running -= 1;
    if (entry.state !== 'running' || entry.controller !== controller) {
      // cancelled while on the wire: the entry is already gone
      this.pump();
      return;
    }
    entry.controller = null;
    switch (outcome.kind) {
      case 'ok':
        this.counters.completed += 1;
        this.counters.bytes += outcome.body.byteLength;
        this.settle(entry, outcome.body);
        break;
      case 'retry':
        this.counters.retries += 1;
        this.sleep(entry, outcome.afterMs, outcome.preparing);
        break;
      case 'failed':
        this.counters.failed += 1;
        this.fail(entry, outcome.error);
        break;
    }
    this.pump();
  }

  private async attempt(entry: Entry, controller: AbortController): Promise<Outcome> {
    const doFetch = this.options.fetch ?? fetch;
    let res: Response;
    try {
      res = await doFetch(entry.url, { cache: 'default', signal: controller.signal });
    } catch (error) {
      if (controller.signal.aborted) return { kind: 'failed', error: abortedError(entry.key) };
      return this.networkFailure(entry, error);
    }
    if (res.ok) {
      try {
        return { kind: 'ok', body: await res.arrayBuffer() };
      } catch (error) {
        if (controller.signal.aborted) return { kind: 'failed', error: abortedError(entry.key) };
        return this.networkFailure(entry, error);
      }
    }
    const { status } = res;
    // drain nothing: the body of an error response is not needed
    void res.body?.cancel().catch(() => undefined);
    if (status === 404 || status === 410) {
      return {
        kind: 'failed',
        error: new AssetError('notFound', entry.key, `HTTP ${String(status)}`, status),
      };
    }
    if (status === 400) {
      return {
        kind: 'failed',
        error: new AssetError('bad', entry.key, 'HTTP 400 (bad path)', status),
      };
    }
    if (status === 401 || status === 403) {
      this.options.log?.warn(
        'assets',
        `${entry.key}: HTTP ${String(status)} (a gated asset without the login cookie?)`,
      );
      return {
        kind: 'failed',
        error: new AssetError('unauthorized', entry.key, `HTTP ${String(status)}`, status),
      };
    }
    if (status === 503 || status === 429) {
      const seconds = parseRetryAfter(res.headers.get('retry-after'));
      if (seconds !== null || status === 429) {
        if (entry.retryAfterTries >= MAX_RETRY_AFTER_TRIES) {
          return {
            kind: 'failed',
            error: new AssetError(
              'server',
              entry.key,
              `HTTP ${String(status)} after ${String(MAX_RETRY_AFTER_TRIES)} waits`,
              status,
            ),
          };
        }
        entry.retryAfterTries += 1;
        const jitter = Math.floor((this.options.random ?? Math.random)() * RETRY_JITTER_MS);
        return { kind: 'retry', afterMs: (seconds ?? 2) * 1000 + jitter, preparing: true };
      }
    }
    if (entry.serverTries < 1) {
      entry.serverTries += 1;
      return { kind: 'retry', afterMs: SERVER_RETRY_MS, preparing: false };
    }
    return {
      kind: 'failed',
      error: new AssetError('server', entry.key, `HTTP ${String(status)}`, status),
    };
  }

  private networkFailure(entry: Entry, error: unknown): Outcome {
    const delay = NETWORK_RETRY_MS[entry.networkTries];
    if (delay === undefined) {
      return { kind: 'failed', error: toAssetError(error, entry.key, 'network') };
    }
    entry.networkTries += 1;
    return { kind: 'retry', afterMs: delay, preparing: false };
  }

  /** Waits `afterMs` outside the concurrency budget, then queues the request again with its old rank. */
  private sleep(entry: Entry, afterMs: number, preparing: boolean): void {
    entry.state = 'waiting';
    if (preparing && !entry.preparing) {
      entry.preparing = true;
      this.options.onWaiting?.(entry.key, true);
    }
    entry.timer = this.timers().setTimeout(() => {
      entry.timer = null;
      if (entry.state !== 'waiting') return;
      entry.state = 'queued';
      this.enqueue(entry);
      this.pump();
    }, afterMs);
  }

  // ---- settling ---------------------------------------------------------------------------------------------

  private finish(entry: Entry): Joiner[] {
    entry.state = 'done';
    entry.queued = false;
    entry.version += 1;
    if (entry.timer !== null) this.timers().clearTimeout(entry.timer);
    entry.timer = null;
    this.entries.delete(entry.key);
    this.options.onPending?.(entry.key, false);
    if (entry.preparing) this.options.onWaiting?.(entry.key, false);
    const joiners = entry.joiners;
    entry.joiners = [];
    for (const j of joiners) {
      if (j.signal && j.onAbort) j.signal.removeEventListener('abort', j.onAbort);
    }
    return joiners;
  }

  private settle(entry: Entry, body: ArrayBuffer): void {
    const joiners = this.finish(entry);
    joiners.forEach((j, i) => {
      j.resolve(i === 0 ? body : body.slice(0));
    });
  }

  private fail(entry: Entry, error: AssetError): void {
    const wasRunning = entry.state === 'running';
    const controller = entry.controller;
    const joiners = this.finish(entry);
    if (wasRunning) controller?.abort();
    for (const j of joiners) j.reject(error);
    if (!wasRunning) this.pump();
  }
}

type Outcome =
  | { kind: 'ok'; body: ArrayBuffer }
  | { kind: 'retry'; afterMs: number; preparing: boolean }
  | { kind: 'failed'; error: AssetError };

function abortedError(key: string): AssetError {
  return new AssetError('aborted', key, 'aborted');
}

function toAssetError(
  error: unknown,
  key: string,
  reason: 'network' | 'server' = 'network',
): AssetError {
  if (error instanceof AssetError) return error;
  if (error instanceof DOMException && error.name === 'AbortError') return abortedError(key);
  return new AssetError(
    reason,
    key,
    error instanceof Error ? error.message : String(error),
    undefined,
    error,
  );
}
