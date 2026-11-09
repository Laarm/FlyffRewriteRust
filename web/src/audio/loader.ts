// `AudioBufferCache` (Plan/44 §6, WP-AUDIO-02): fetch, decode, cache and preload the sound-effect files.
//
//   get(path, priority) -> Promise<AudioBuffer | null>
//     missing.has(path)  -> null                        a file that does not exist is never requested again (KQ-30)
//     decoded.has(path)  -> the buffer (LRU touch)
//     inflight.has(path) -> the same promise            two requests of one path issue ONE fetch
//     else fetch the bytes (the asset manager's one-shot fetch at `audio-demand` / `audio-prefetch`), decode them with
//     `decodeAudioData` (at most four at a time; a `.wav` it rejects goes through `parsePcmWav`), reject a stereo file
//     ("not mono": the legacy `CWaveFile` refused it), insert into the LRU and evict.
//
// The budget is accounted as `length * channels * 4` bytes (float32 at the context rate). Eviction takes the least recently
// played buffer that is neither pinned (the core set of §6.5) nor in use by a voice. The encoded bytes are not kept: a
// re-decode after an eviction is one HTTP-cache hit plus CPU (D3: no IndexedDB / CacheStorage).

import type { Logger } from '../platform/log';
import type { AudioBufferLike, AudioEngine } from './engine';
import { parsePcmWav, wavToAudioBuffer } from './wav';

export type BufferPriority = 'demand' | 'prefetch';

/** `decodedBudgetBytes`: 96 MiB, 48 MiB on a low-memory or coarse-pointer device (44 §6.4). */
export const DECODED_BUDGET_BYTES = 96 * 1024 * 1024;
export const DECODED_BUDGET_BYTES_LOW = 48 * 1024 * 1024;
/** At most this many `decodeAudioData` calls at a time (44 §6.1). */
export const DECODE_CONCURRENCY = 4;

export interface BufferCacheDeps {
  readonly engine: AudioEngine;
  /** The one-shot byte fetch (the asset manager's `fetchBytes` at `Prio.AudioDemand` / `Prio.AudioPrefetch`). */
  readonly fetchBytes: (path: string, priority: BufferPriority) => Promise<ArrayBuffer>;
  /** `true` when a fetch error says the file does not exist for this client (`AssetError.missing`); default: `.missing === true`. */
  readonly isMissingError?: (error: unknown) => boolean;
  readonly log: Logger;
  readonly budgetBytes?: number;
  readonly decodeConcurrency?: number;
}

interface Entry {
  readonly buffer: AudioBufferLike;
  readonly bytes: number;
  lastUse: number;
  pinned: boolean;
  /** Voices that are playing this buffer: never evicted while > 0. */
  voices: number;
}

interface Pending {
  readonly promise: Promise<AudioBufferLike | null>;
  resolve(value: AudioBufferLike | null): void;
  priority: BufferPriority;
  settled: boolean;
}

function defaultIsMissing(error: unknown): boolean {
  return (
    typeof error === 'object' && error !== null && (error as { missing?: unknown }).missing === true
  );
}

/** `pending.settled` read through a call: TypeScript does not narrow it across an `await`, but another request may have settled it. */
function isSettled(p: Pending): boolean {
  return p.settled;
}

function bytesOf(buffer: AudioBufferLike): number {
  return buffer.length * (buffer.numberOfChannels ?? 1) * 4;
}

export class AudioBufferCache {
  readonly #deps: BufferCacheDeps;
  readonly #entries = new Map<string, Entry>();
  readonly #inflight = new Map<string, Pending>();
  readonly #pinned = new Set<string>();
  readonly #missing = new Set<string>();
  readonly #loggedMissing = new Set<string>();
  readonly #budget: number;
  readonly #concurrency: number;
  #active = 0;
  readonly #waiting: (() => void)[] = [];
  #clock = 0;

  constructor(deps: BufferCacheDeps) {
    this.#deps = deps;
    this.#budget = deps.budgetBytes ?? DECODED_BUDGET_BYTES;
    this.#concurrency = Math.max(1, deps.decodeConcurrency ?? DECODE_CONCURRENCY);
  }

  // ---- queries --------------------------------------------------------------------------------------------------

  /** The paths that do not exist for this client (page lifetime). */
  get missing(): readonly string[] {
    return [...this.#missing];
  }
  isMissing(path: string): boolean {
    return this.#missing.has(path);
  }
  /** Decoded buffers in the cache. */
  get size(): number {
    return this.#entries.size;
  }
  get decodedBytes(): number {
    return this.#deps.engine.counters.decodedBytes;
  }
  get budgetBytes(): number {
    return this.#budget;
  }
  get inflight(): number {
    return this.#inflight.size;
  }
  /** The paths of the resident buffers (the probe / tests). */
  get residentPaths(): readonly string[] {
    return [...this.#entries.keys()];
  }
  /** `true` when the decoded buffer is resident. */
  has(path: string): boolean {
    return this.#entries.has(path);
  }
  isPinned(path: string): boolean {
    return this.#pinned.has(path);
  }
  /** The pinned paths (the core set). */
  get pinned(): readonly string[] {
    return [...this.#pinned];
  }

  /** The decoded buffer, or `undefined` while it is not resident (a hit counts as a use for the LRU). */
  peek(path: string): AudioBufferLike | undefined {
    const e = this.#entries.get(path);
    if (e === undefined) return undefined;
    e.lastUse = ++this.#clock;
    return e.buffer;
  }

  /** A voice starts playing `path`: the buffer cannot be evicted until the returned function is called. */
  retain(path: string): () => void {
    const e = this.#entries.get(path);
    if (e === undefined) return () => undefined;
    e.voices += 1;
    e.lastUse = ++this.#clock;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      // the entry may have been replaced by a re-decode: release on the current one only
      const cur = this.#entries.get(path);
      if (cur === e && e.voices > 0) e.voices -= 1;
    };
  }

  // ---- loading --------------------------------------------------------------------------------------------------

  /** Fetch + decode (or the resident buffer). Resolves `null` for a missing, undecodable or stereo file. Never rejects. */
  get(path: string, priority: BufferPriority = 'demand'): Promise<AudioBufferLike | null> {
    if (this.#missing.has(path)) return Promise.resolve(null);
    const e = this.#entries.get(path);
    if (e !== undefined) {
      e.lastUse = ++this.#clock;
      return Promise.resolve(e.buffer);
    }
    const pending = this.#inflight.get(path);
    if (pending !== undefined) {
      if (priority === 'demand' && pending.priority === 'prefetch') {
        // a play request is waiting for a file that is only queued as a prefetch: ask again at the higher priority; the
        // first of the two results settles the shared promise
        pending.priority = 'demand';
        void this.#attempt(path, pending, 'demand');
      }
      return pending.promise;
    }
    let resolve: Pending['resolve'] = () => undefined;
    const promise = new Promise<AudioBufferLike | null>((r) => {
      resolve = r;
    });
    const created: Pending = { promise, resolve, priority, settled: false };
    this.#inflight.set(path, created);
    void this.#attempt(path, created, priority);
    return promise;
  }

  /** A low-priority load that nobody waits for. */
  prefetch(path: string): void {
    if (this.#missing.has(path) || this.#entries.has(path) || this.#inflight.has(path)) return;
    void this.get(path, 'prefetch');
  }

  /**
   * Adds `paths` to the pinned set (never evicted) and loads the ones that are not resident. The promise resolves when every
   * load settled (a missing file settles to nothing).
   */
  pin(paths: Iterable<string>, priority: BufferPriority = 'prefetch'): Promise<void> {
    const loads: Promise<unknown>[] = [];
    for (const path of paths) {
      this.#pinned.add(path);
      const e = this.#entries.get(path);
      if (e !== undefined) e.pinned = true;
      else if (!this.#missing.has(path)) loads.push(this.get(path, priority));
    }
    return Promise.all(loads).then(() => undefined);
  }

  /** Drops every decoded buffer that no voice uses (a context loss, dispose). Pinned entries are kept unless `all`. */
  clear(all = false): void {
    for (const [path, e] of [...this.#entries]) {
      if (e.voices > 0 || (e.pinned && !all)) continue;
      this.#drop(path, e);
    }
  }

  dispose(): void {
    this.#entries.clear();
    this.#inflight.clear();
    this.#deps.engine.counters.decodedBytes = 0;
  }

  // ---- internals ------------------------------------------------------------------------------------------------

  async #attempt(path: string, pending: Pending, priority: BufferPriority): Promise<void> {
    const { engine, log } = this.#deps;
    let bytes: ArrayBuffer;
    try {
      bytes = await this.#deps.fetchBytes(path, priority);
    } catch (error) {
      if (pending.settled) return;
      const isMissing = (this.#deps.isMissingError ?? defaultIsMissing)(error);
      if (isMissing) this.#markMissing(path, 'not found');
      else {
        engine.noteError('sound-fetch', error instanceof Error ? error.message : String(error));
        log.debug('audio', `sound ${path} could not be fetched; it may be requested again`);
      }
      this.#settle(path, pending, null);
      return;
    }
    if (pending.settled) return;
    const buffer = await this.#decode(path, bytes);
    if (isSettled(pending)) return;
    if (buffer === null) {
      this.#settle(path, pending, null);
      return;
    }
    if ((buffer.numberOfChannels ?? 1) > 1) {
      // KEEP: the legacy reader accepted mono PCM only
      this.#markMissing(path, 'not mono');
      this.#settle(path, pending, null);
      return;
    }
    this.#insert(path, buffer);
    this.#settle(path, pending, buffer);
  }

  #settle(path: string, pending: Pending, value: AudioBufferLike | null): void {
    if (pending.settled) return;
    pending.settled = true;
    if (this.#inflight.get(path) === pending) this.#inflight.delete(path);
    pending.resolve(value);
  }

  #markMissing(path: string, why: string): void {
    this.#missing.add(path);
    if (!this.#loggedMissing.has(path)) {
      this.#loggedMissing.add(path);
      this.#deps.log.debug('audio', `sound ${path} is missing (${why})`);
    }
  }

  async #decode(path: string, bytes: ArrayBuffer): Promise<AudioBufferLike | null> {
    const engine = this.#deps.engine;
    const ctx = engine.context;
    if (ctx === null) return null;
    await this.#acquire();
    try {
      try {
        // decodeAudioData detaches its input: hand it a copy, the original stays for the fallback
        const buffer = await ctx.decodeAudioData(bytes.slice(0));
        engine.counters.decodes += 1;
        return buffer;
      } catch (error) {
        if (!path.endsWith('.wav')) {
          engine.counters.decodeFailures += 1;
          this.#markMissing(path, 'undecodable');
          engine.noteError('sound-decode', error instanceof Error ? error.message : String(error));
          return null;
        }
      }
      // the PCM fallback of §6.3
      const wav = parsePcmWav(bytes);
      const buffer = wav === null ? null : wavToAudioBuffer(ctx, wav);
      if (buffer === null) {
        engine.counters.decodeFailures += 1;
        this.#markMissing(path, 'undecodable');
        return null;
      }
      engine.counters.decodeFallbacks += 1;
      return buffer;
    } finally {
      this.#release();
    }
  }

  #acquire(): Promise<void> {
    if (this.#active < this.#concurrency) {
      this.#active += 1;
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      this.#waiting.push(() => {
        this.#active += 1;
        resolve();
      });
    });
  }

  #release(): void {
    this.#active -= 1;
    const next = this.#waiting.shift();
    if (next !== undefined) next();
  }

  #insert(path: string, buffer: AudioBufferLike): void {
    const counters = this.#deps.engine.counters;
    const old = this.#entries.get(path);
    if (old !== undefined) counters.decodedBytes -= old.bytes;
    const entry: Entry = {
      buffer,
      bytes: bytesOf(buffer),
      lastUse: ++this.#clock,
      pinned: this.#pinned.has(path),
      voices: old?.voices ?? 0,
    };
    this.#entries.set(path, entry);
    counters.decodedBytes += entry.bytes;
    this.#evict();
  }

  #drop(path: string, e: Entry): void {
    this.#entries.delete(path);
    this.#deps.engine.counters.decodedBytes -= e.bytes;
  }

  /** Least-recently-played first, never a pinned buffer or one with an active voice. */
  #evict(): void {
    const counters = this.#deps.engine.counters;
    while (counters.decodedBytes > this.#budget) {
      let victimPath: string | null = null;
      let victim: Entry | null = null;
      for (const [path, e] of this.#entries) {
        if (e.pinned || e.voices > 0) continue;
        if (victim === null || e.lastUse < victim.lastUse) {
          victim = e;
          victimPath = path;
        }
      }
      if (victim === null || victimPath === null) return;
      this.#drop(victimPath, victim);
      counters.evictions += 1;
    }
  }
}
