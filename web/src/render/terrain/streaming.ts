// Terrain streaming (Plan/41 section 9.8, WP-REND-08, PL-131; ref/19 sections 16.1, 23.5, ref/21 section 11.3): which lands
// to load around the player, which to keep, which to prefetch, and the asynchronous loads with eviction.
//
//   r      = max(1, floor(512 / (128 * mpu)))        1 at MPU 4 (3 x 3 lands), 2 at MPU 2, 4 at MPU 1 (the far plane)
//   window = lands [x-r .. x+r] x [z-r .. z+r]       around the PLAYER's land (the original's `ReadWorld`)
//   retain = lands [x-r-1 .. x+r+1] x [..]           (2r+3)^2: the hysteresis that avoids reloading when walking back
//   prefetch  within 64 m of a land edge the 2r+1 lands of the next row/column beyond the window on that side
//   gate      the inner 3 x 3 PRESENT lands (Plan/40 9.8): the loading gate opens when they are all ready
//
// The window is evaluated every 250 ms and on every land change of the player (`update`). Only lands listed in
// `present` have a `.lnd`; the others render nothing (their collision chunk is doc 40's business). Loads are asynchronous
// and never block a frame: a land that is not ready yet is simply not drawn and fades in when it is. The collision half
// of `WorldStreamer` (Plan/40 9.5, WP-CLIENT-21) is not here; it can use `windowLands`/`retainLands` of this file.

import { Prio } from '../../workers/priority';
import { AssetNotFoundError } from '../world/ports';

export interface LandKey {
  readonly x: number;
  readonly z: number;
}

/** `r = max(1, floor(512 / (128 * mpu)))` (Plan/40 9.5). */
export function visibilityRadius(mpu: number): number {
  return Math.max(1, Math.floor(512 / (128 * mpu)));
}

/** The land that contains world position `(x, z)` (LH world units), unclamped. */
export function landOf(x: number, z: number, mpu: number): LandKey {
  const edge = 128 * mpu;
  return { x: Math.floor(x / edge), z: Math.floor(z / edge) };
}

/** The lands of the square around `center` with Chebyshev radius `radius`, clipped to the world, row by row. */
export function squareLands(
  center: LandKey,
  radius: number,
  landsW: number,
  landsH: number,
): LandKey[] {
  const out: LandKey[] = [];
  for (let z = center.z - radius; z <= center.z + radius; z++) {
    for (let x = center.x - radius; x <= center.x + radius; x++) {
      if (x >= 0 && z >= 0 && x < landsW && z < landsH) out.push({ x, z });
    }
  }
  return out;
}

export const windowLands = (c: LandKey, r: number, w: number, h: number): LandKey[] =>
  squareLands(c, r, w, h);
export const retainLands = (c: LandKey, r: number, w: number, h: number): LandKey[] =>
  squareLands(c, r + 1, w, h);

/** Metres from a land edge below which the next row/column is prefetched (ref/21 port note 6). */
export const PREFETCH_DISTANCE = 64;

/**
 * The lands to prefetch: when the player is within `PREFETCH_DISTANCE` of an edge of its land, the `2r + 1` lands of the next
 * row/column beyond the window on that side (and the corner land when two edges are close).
 */
export function prefetchLands(
  pos: { readonly x: number; readonly z: number },
  mpu: number,
  landsW: number,
  landsH: number,
): LandKey[] {
  const edge = 128 * mpu;
  const c = landOf(pos.x, pos.z, mpu);
  const r = visibilityRadius(mpu);
  const lx = pos.x - c.x * edge;
  const lz = pos.z - c.z * edge;
  const west = lx < PREFETCH_DISTANCE;
  const east = edge - lx < PREFETCH_DISTANCE;
  const south = lz < PREFETCH_DISTANCE;
  const north = edge - lz < PREFETCH_DISTANCE;
  const out: LandKey[] = [];
  const push = (x: number, z: number): void => {
    if (x >= 0 && z >= 0 && x < landsW && z < landsH) out.push({ x, z });
  };
  if (east) for (let z = c.z - r; z <= c.z + r; z++) push(c.x + r + 1, z);
  if (west) for (let z = c.z - r; z <= c.z + r; z++) push(c.x - r - 1, z);
  if (north) for (let x = c.x - r; x <= c.x + r; x++) push(x, c.z + r + 1);
  if (south) for (let x = c.x - r; x <= c.x + r; x++) push(x, c.z - r - 1);
  if (east && north) push(c.x + r + 1, c.z + r + 1);
  if (east && south) push(c.x + r + 1, c.z - r - 1);
  if (west && north) push(c.x - r - 1, c.z + r + 1);
  if (west && south) push(c.x - r - 1, c.z - r - 1);
  return out;
}

export type LandState = 'absent' | 'loading' | 'ready' | 'failed';

/** What the streamer asks of its owner. */
export interface StreamerHooks<D> {
  /** Starts fetching and decoding land `(x, z)`; `signal` aborts when the land leaves the retain window. */
  load(req: {
    x: number;
    z: number;
    prio: Prio;
    distance: number;
    signal: AbortSignal;
  }): Promise<D>;
  /**
   * Builds the GPU objects of a decoded land; resolves when the land is READY (uploaded). Called at most once per
   * successful load; `isCurrent()` turns false when the land was evicted meanwhile (the owner should drop its work).
   */
  build(x: number, z: number, data: D, isCurrent: () => boolean): Promise<void>;
  /** The land left the retain window (or the world): release it. Called only for lands that were built. */
  evict(x: number, z: number): void;
  /** A land failed to load; `notFound` marks a missing file. */
  failed?(x: number, z: number, error: unknown): void;
}

interface Slot<D> {
  state: LandState;
  prio: Prio;
  abort: AbortController | null;
  /** A decoded land waits for `build`. */
  data: D | null;
  retryAt: number;
  /** The file does not exist (a final failure: the land counts as absent, not as missing from the gate). */
  missing: boolean;
}

export interface StreamerStats {
  readonly requested: number;
  readonly decoded: number;
  readonly built: number;
  readonly evicted: number;
  readonly failed: number;
  readonly inflight: number;
}

export interface StreamerOptions {
  /** Loads running at the same time (the decode pool schedules by priority itself). */
  readonly maxInflight?: number;
  /** A failed land is retried after this many ms (Plan/40 9.7: 10 s). */
  readonly retryMs?: number;
  /** Minimum ms between two evaluations of the window for an unchanged player land. */
  readonly intervalMs?: number;
}

/** The terrain streamer of one world. */
export class TerrainStreamer<D> {
  private readonly slots = new Map<number, Slot<D>>();
  private center: LandKey | null = null;
  private lastEval = -Infinity;
  private lastPos: { x: number; z: number } | null = null;
  private inflight = 0;
  private readonly queue: { key: number; prio: Prio; distance: number }[] = [];
  private requested = 0;
  private decoded = 0;
  private built = 0;
  private evicted = 0;
  private failedCount = 0;
  private disposed = false;
  readonly r: number;
  private readonly maxInflight: number;
  private readonly retryMs: number;
  private readonly intervalMs: number;

  constructor(
    readonly landsW: number,
    readonly landsH: number,
    readonly mpu: number,
    /** Land indices (`x + z * landsW`) that have a `.lnd`. */
    private readonly present: ReadonlySet<number>,
    private readonly hooks: StreamerHooks<D>,
    options: StreamerOptions = {},
  ) {
    this.r = visibilityRadius(mpu);
    this.maxInflight = options.maxInflight ?? 3;
    this.retryMs = options.retryMs ?? 10_000;
    this.intervalMs = options.intervalMs ?? 250;
  }

  private key(x: number, z: number): number {
    return x + z * this.landsW;
  }

  hasFile(x: number, z: number): boolean {
    return this.present.has(this.key(x, z));
  }

  state(x: number, z: number): LandState {
    return this.slots.get(this.key(x, z))?.state ?? 'absent';
  }

  /** Ready lands as keys. */
  readyLands(): LandKey[] {
    const out: LandKey[] = [];
    for (const [k, s] of this.slots) {
      if (s.state === 'ready') out.push({ x: k % this.landsW, z: Math.floor(k / this.landsW) });
    }
    return out;
  }

  get readyCount(): number {
    let n = 0;
    for (const s of this.slots.values()) if (s.state === 'ready') n++;
    return n;
  }

  stats(): StreamerStats {
    return {
      requested: this.requested,
      decoded: this.decoded,
      built: this.built,
      evicted: this.evicted,
      failed: this.failedCount,
      inflight: this.inflight,
    };
  }

  /** The gate set: the inner 3 x 3 around the player's land that have a file. */
  gateLands(): LandKey[] {
    if (!this.center) return [];
    return squareLands(this.center, 1, this.landsW, this.landsH).filter(
      (l) => this.hasFile(l.x, l.z) && this.slots.get(this.key(l.x, l.z))?.missing !== true,
    );
  }

  /** Ready/wanted of the gate set (`progress` = ready / wanted, 1 when nothing is wanted). */
  gate(): { ready: number; wanted: number; open: boolean; progress: number } {
    const lands = this.gateLands();
    const ready = lands.filter((l) => this.state(l.x, l.z) === 'ready').length;
    return {
      ready,
      wanted: lands.length,
      open: ready === lands.length,
      progress: lands.length === 0 ? 1 : ready / lands.length,
    };
  }

  /**
   * Re-evaluates the window around the player's world position `(x, z)` (LH). Does real work when the player's land
   * changed, when `force` is set or when 250 ms passed since the last evaluation.
   */
  update(pos: { readonly x: number; readonly z: number }, nowMs: number, force = false): void {
    if (this.disposed) return;
    const c = landOf(pos.x, pos.z, this.mpu);
    const clamped: LandKey = {
      x: Math.min(this.landsW - 1, Math.max(0, c.x)),
      z: Math.min(this.landsH - 1, Math.max(0, c.z)),
    };
    const changed = this.center?.x !== clamped.x || this.center.z !== clamped.z;
    if (!force && !changed && nowMs - this.lastEval < this.intervalMs) {
      this.retryFailed(nowMs);
      return;
    }
    this.lastEval = nowMs;
    this.center = clamped;
    this.lastPos = { x: pos.x, z: pos.z };
    const gate = new Set(
      squareLands(clamped, 1, this.landsW, this.landsH).map((l) => this.key(l.x, l.z)),
    );
    const edge = 128 * this.mpu;
    const dist = (l: LandKey): number =>
      Math.hypot((l.x + 0.5) * edge - pos.x, (l.z + 0.5) * edge - pos.z);

    // the window: the gate set at Critical, the rest at Near
    for (const l of windowLands(clamped, this.r, this.landsW, this.landsH)) {
      if (!this.hasFile(l.x, l.z)) continue;
      const k = this.key(l.x, l.z);
      this.want(l, gate.has(k) ? Prio.Critical : Prio.Near, dist(l));
    }
    // the prefetch ring
    for (const l of prefetchLands(pos, this.mpu, this.landsW, this.landsH)) {
      if (this.hasFile(l.x, l.z)) this.want(l, Prio.Prefetch, dist(l));
    }
    // eviction outside the retain window
    const keep = new Set(
      retainLands(clamped, this.r, this.landsW, this.landsH).map((l) => this.key(l.x, l.z)),
    );
    // lands within the prefetch ring are kept as well (they were requested on purpose)
    for (const l of prefetchLands(pos, this.mpu, this.landsW, this.landsH)) {
      keep.add(this.key(l.x, l.z));
    }
    for (const [k, s] of [...this.slots]) {
      if (keep.has(k)) continue;
      this.drop(k, s);
    }
    this.retryFailed(nowMs);
    this.pump();
  }

  private want(l: LandKey, prio: Prio, distance: number): void {
    const k = this.key(l.x, l.z);
    const s = this.slots.get(k);
    if (s) {
      // a stronger wish raises the priority of a queued request
      if (prio < s.prio) {
        s.prio = prio;
        const q = this.queue.find((e) => e.key === k);
        if (q) {
          q.prio = prio;
          q.distance = distance;
        }
      }
      return;
    }
    this.slots.set(k, {
      state: 'loading',
      prio,
      abort: null,
      data: null,
      retryAt: 0,
      missing: false,
    });
    this.queue.push({ key: k, prio, distance });
    this.requested++;
  }

  private retryFailed(nowMs: number): void {
    for (const [k, s] of this.slots) {
      if (s.state === 'failed' && s.retryAt > 0 && nowMs >= s.retryAt) {
        s.state = 'loading';
        s.retryAt = 0;
        this.queue.push({ key: k, prio: s.prio, distance: 0 });
      }
    }
    this.pump();
  }

  private drop(k: number, s: Slot<D>): void {
    const x = k % this.landsW;
    const z = Math.floor(k / this.landsW);
    s.abort?.abort();
    const qi = this.queue.findIndex((e) => e.key === k);
    if (qi >= 0) this.queue.splice(qi, 1);
    this.slots.delete(k);
    // a land that is still being built cleans up after itself when `isCurrent()` turns false
    if (s.state === 'ready') {
      this.evicted++;
      this.hooks.evict(x, z);
    }
  }

  private pump(): void {
    while (this.inflight < this.maxInflight && this.queue.length > 0) {
      this.queue.sort((a, b) => a.prio - b.prio || a.distance - b.distance);
      const next = this.queue.shift();
      if (!next) break;
      const slot = this.slots.get(next.key);
      if (slot?.state !== 'loading' || slot.abort) continue;
      this.start(next.key, slot, next.distance);
    }
  }

  private start(k: number, slot: Slot<D>, distance: number): void {
    const x = k % this.landsW;
    const z = Math.floor(k / this.landsW);
    const abort = new AbortController();
    slot.abort = abort;
    this.inflight++;
    const current = (): boolean => this.slots.get(k) === slot && !abort.signal.aborted;
    this.hooks
      .load({ x, z, prio: slot.prio, distance, signal: abort.signal })
      .then(async (data) => {
        this.decoded++;
        if (!current()) return;
        slot.data = data;
        await this.hooks.build(x, z, data, current);
        if (!current()) return;
        slot.data = null;
        slot.state = 'ready';
        slot.abort = null;
        this.built++;
      })
      .catch((error: unknown) => {
        if (!current()) return;
        slot.state = 'failed';
        slot.abort = null;
        slot.data = null;
        // a missing file is final; anything else is retried (Plan/40 9.7)
        slot.missing = error instanceof AssetNotFoundError;
        slot.retryAt = slot.missing ? 0 : this.lastEval + this.retryMs;
        this.failedCount++;
        this.hooks.failed?.(x, z, error);
      })
      .finally(() => {
        this.inflight--;
        this.pump();
      });
  }

  /** Drops everything (world change, dispose): aborts loads and evicts the built lands. */
  clear(): void {
    for (const [k, s] of [...this.slots]) this.drop(k, s);
    this.queue.length = 0;
    this.center = null;
    this.lastPos = null;
  }

  dispose(): void {
    this.clear();
    this.disposed = true;
  }

  /** The player position of the last evaluation. */
  get position(): { x: number; z: number } | null {
    return this.lastPos;
  }

  get centerLand(): LandKey | null {
    return this.center;
  }
}
