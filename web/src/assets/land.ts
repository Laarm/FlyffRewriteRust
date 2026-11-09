// WorldStreamer (Plan/40 section 9.5, CA12, WP-CLIENT-21; ref/21 section 11.3, port note 6; `src:_Common/WorldFile.cpp:219,
// 977-1019`, PL-131): the COLLISION half of world streaming. The window `(2r+1)^2` around the player's land is loaded and the
// ring `(2r+3)^2` retained, `r = max(1, floor(512 / (128 * MPU)))` (3x3/5x5 at MPU 4, 5x5/7x7 at MPU 2, 9x9/11x11 at MPU 1,
// the original `m_nVisibilityLand`). EVERY land coordinate of the world has a collision chunk (24 section 10.7: a land without
// a `.lnd` gets the uniform chunk), so the chunk of every window land is requested, present or not: the sim has no API that
// marks a land `Missing`, and a land that stays `Pending` makes movers use their own `y` instead of the server's 0 (31 10.1).
// Terrain (`.lnd`) is streamed by the renderer (`render/terrain/streaming.ts`, the same window rule, only for lands that
// exist) and reaches the gate through `RenderApi.worldGate()`.
//
//   * a chunk is fetched through the asset manager (`gen:collision:<folder>/<x>-<z>`, the `chunk_check` worker job validates
//     it) and handed to the sim as a `chunk` load (cap L); the asset handle is released once the bytes are queued;
//   * lands that leave the retained ring are unloaded (`unload_chunk`: the sim keeps them `Pending`);
//   * within 64 m of a land edge the 2r+1 lands beyond the window edge on that side are prefetched (priority `Prefetch`);
//   * `ChunkMissing{lx, lz}` of the sim raises that chunk to `Critical`, even outside the window;
//   * a failed land is retried every 10 s while it is in the window.
//
// The streamer is driven by `update(x, z, nowMs)`; the world module calls it from a tick system (at most every 250 ms and on
// every land change) and once at `WorldReadInfo` with the position of the join.

import { Prio } from '../workers/priority';
import type { AssetHandle } from './handles';
import type { AssetManager } from './manager';
import { ChunkType } from './poseTypes';

/** Land edge length is `LAND_UNITS * mpu` world units (128 cells of `mpu` units). */
export const LAND_UNITS = 128;
/** Far plane of the original client: the window covers it (`m_fFarPlane`). */
export const FAR_PLANE = 512;
/** The prefetch distance to a land edge (ref/21 port note 6). */
export const PREFETCH_DISTANCE = 64;
/** The retry period of a failed land. */
export const RETRY_MS = 10_000;
/** The update cadence (a land change updates at once). */
export const UPDATE_MS = 250;

/** What the streamer needs to know about the world (`ClientManifest.worlds[folder]` + `.wld`). */
export interface StreamWorld {
  readonly worldId: number;
  /** The folder as in the asset manifest (`gen:collision:<folder>/..`). */
  readonly folder: string;
  readonly mpu: number;
  readonly landsW: number;
  readonly landsH: number;
}

/** `r = max(1, floor(512 / (128 * mpu)))`. */
export function windowRadius(mpu: number): number {
  return Math.max(1, Math.floor(FAR_PLANE / (LAND_UNITS * Math.max(1, mpu))));
}

/** The land `(x, z)` of a position (`floor(pos / (128 * mpu))`, not clamped). */
export function landOf(posX: number, posZ: number, mpu: number): { x: number; z: number } {
  const edge = LAND_UNITS * mpu;
  return { x: Math.floor(posX / edge), z: Math.floor(posZ / edge) };
}

export type ChunkState = 'fetching' | 'queued' | 'ready' | 'failed';

interface Land {
  readonly x: number;
  readonly z: number;
  state: ChunkState;
  /** The fetch handles (a Critical re-request joins the first one). */
  handles: AssetHandle<Uint8Array>[];
  failedAt: number;
  /** Generation of the world the request belongs to. */
  readonly epoch: number;
}

/** The sim side of the streamer: the bridge's load queue. */
export interface StreamerSim {
  enqueueLoad(
    load:
      | { kind: 'chunk'; lx: number; lz: number; bytes: Uint8Array; prio: number }
      | { kind: 'unchunk'; lx: number; lz: number; prio: number },
  ): void;
}

export interface StreamerDeps {
  readonly assets: Pick<AssetManager, 'acquire'>;
  readonly sim: StreamerSim;
  readonly now: () => number;
  /** `true` while the loading gate waits (the gate set is requested at Critical). */
  readonly loading: () => boolean;
  readonly log?: { warn(scope: string, message: string): void };
}

export interface StreamerStats {
  /** Collision chunks the sim has loaded. */
  readonly chunksReady: number;
  /** Chunks in flight or queued for the sim. */
  readonly chunksPending: number;
  readonly chunksFailed: number;
  /** Requests made since the world started. */
  readonly requests: number;
  /** Requests made at `Prefetch` priority. */
  readonly prefetches: number;
  readonly unloads: number;
  readonly retries: number;
  readonly window: number;
  readonly retain: number;
}

/** What the loading gate reads. */
export interface CollisionGate {
  /** Lands of the window whose chunk is loaded in the sim / lands of the window. */
  readonly ready: number;
  readonly wanted: number;
}

const key = (x: number, z: number): number => x * 65536 + z;

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === 'string' ? e : 'unknown error';
}

export class WorldStreamer {
  private world: StreamWorld | null = null;
  private readonly lands = new Map<number, Land>();
  private epoch = 0;
  private center: { x: number; z: number } | null = null;
  private lastUpdateAt = Number.NEGATIVE_INFINITY;
  private requests = 0;
  private prefetches = 0;
  private unloads = 0;
  private retries = 0;
  private windowSize = 0;
  private retainSize = 0;

  constructor(private readonly deps: StreamerDeps) {}

  /** Starts (or switches) the world: everything of the previous one is released and unloaded in the sim by `set_world`. */
  setWorld(world: StreamWorld | null, posX = 0, posZ = 0): void {
    this.releaseAll();
    this.world = world;
    this.center = null;
    this.lastUpdateAt = Number.NEGATIVE_INFINITY;
    this.requests = 0;
    this.prefetches = 0;
    this.unloads = 0;
    this.retries = 0;
    this.epoch += 1;
    if (world) this.update(posX, posZ, this.deps.now());
  }

  get current(): StreamWorld | null {
    return this.world;
  }

  private releaseAll(): void {
    for (const land of this.lands.values()) for (const h of land.handles) h.release();
    this.lands.clear();
  }

  /**
   * Recomputes the window around `(posX, posZ)`. Cheap to call every step: nothing happens unless the land changed, 250 ms
   * passed, or a failed land is due for a retry.
   */
  update(posX: number, posZ: number, nowMs: number): void {
    const w = this.world;
    if (!w) return;
    const here = landOf(posX, posZ, w.mpu);
    const c = this.center;
    const changed = c?.x !== here.x || c.z !== here.z;
    if (!changed && nowMs - this.lastUpdateAt < UPDATE_MS) return;
    this.lastUpdateAt = nowMs;
    this.center = here;
    const r = windowRadius(w.mpu);
    const inWorld = (x: number, z: number): boolean =>
      x >= 0 && z >= 0 && x < w.landsW && z < w.landsH;
    const critical = this.deps.loading();

    // the window: every land, present or not
    let windowCount = 0;
    for (let z = here.z - r; z <= here.z + r; z += 1) {
      for (let x = here.x - r; x <= here.x + r; x += 1) {
        if (!inWorld(x, z)) continue;
        windowCount += 1;
        const dist = Math.hypot(x - here.x, z - here.z);
        this.ensure(x, z, critical ? Prio.Critical : Prio.Near, dist, nowMs, false);
      }
    }
    this.windowSize = windowCount;

    // border prefetch: the next row / column on the side the player approaches
    const edge = LAND_UNITS * w.mpu;
    const lx = posX - here.x * edge;
    const lz = posZ - here.z * edge;
    const sides: { dx: number; dz: number }[] = [];
    if (lx < PREFETCH_DISTANCE) sides.push({ dx: -1, dz: 0 });
    if (edge - lx < PREFETCH_DISTANCE) sides.push({ dx: 1, dz: 0 });
    if (lz < PREFETCH_DISTANCE) sides.push({ dx: 0, dz: -1 });
    if (edge - lz < PREFETCH_DISTANCE) sides.push({ dx: 0, dz: 1 });
    for (const s of sides) {
      for (let k = -r; k <= r; k += 1) {
        const x = s.dx !== 0 ? here.x + s.dx * (r + 1) : here.x + k;
        const z = s.dz !== 0 ? here.z + s.dz * (r + 1) : here.z + k;
        if (inWorld(x, z)) this.ensure(x, z, Prio.Prefetch, r + 1, nowMs, true);
      }
    }

    // retention: the ring of one more land stays, everything beyond is released and unloaded in the sim
    let retained = 0;
    for (const [k, land] of this.lands) {
      const inRetain = Math.abs(land.x - here.x) <= r + 1 && Math.abs(land.z - here.z) <= r + 1;
      if (inRetain) {
        retained += 1;
        continue;
      }
      this.drop(land);
      this.lands.delete(k);
    }
    this.retainSize = retained;
  }

  /** Requests the chunk of a land unless it is known; failed lands are retried after {@link RETRY_MS}. */
  private ensure(
    x: number,
    z: number,
    prio: Prio,
    distance: number,
    nowMs: number,
    prefetch: boolean,
  ): void {
    const w = this.world;
    if (!w) return;
    const k = key(x, z);
    const known = this.lands.get(k);
    if (known) {
      if (known.state === 'failed' && nowMs - known.failedAt >= RETRY_MS) {
        this.retries += 1;
        this.lands.delete(k);
        this.request(x, z, prio, distance, prefetch);
        return;
      }
      // a land that was prefetched and is now in the window while the gate waits: raise it (joins the fetch)
      if (known.state === 'fetching' && prio === Prio.Critical) this.raise(known, distance);
      return;
    }
    this.request(x, z, prio, distance, prefetch);
  }

  private request(x: number, z: number, prio: Prio, distance: number, prefetch: boolean): void {
    const w = this.world;
    if (!w) return;
    const land: Land = { x, z, state: 'fetching', handles: [], failedAt: 0, epoch: this.epoch };
    this.lands.set(key(x, z), land);
    this.requests += 1;
    if (prefetch) this.prefetches += 1;
    const handle = this.deps.assets.acquire(
      ChunkType,
      { ns: 'gen', kind: 'collision', folder: w.folder, x, z },
      prio,
      distance,
      { scoped: true },
    );
    land.handles.push(handle);
    handle.ready.then(
      (bytes) => {
        if (this.lands.get(key(x, z)) !== land || land.epoch !== this.epoch) return;
        land.state = 'queued';
        this.deps.sim.enqueueLoad({ kind: 'chunk', lx: x, lz: z, bytes, prio });
        for (const h of land.handles) h.release(); // the sim copies the bytes
        land.handles = [];
      },
      (error: unknown) => {
        if (this.lands.get(key(x, z)) !== land || land.epoch !== this.epoch) return;
        const aborted = (error as { reason?: unknown }).reason === 'aborted';
        if (aborted) return;
        land.state = 'failed';
        land.failedAt = this.deps.now();
        for (const h of land.handles) h.release();
        land.handles = [];
        this.deps.log?.warn(
          'world',
          `collision chunk ${w.folder}/${String(x)}-${String(z)}: ${errorText(error)}`,
        );
      },
    );
  }

  private raise(land: Land, distance: number): void {
    const w = this.world;
    if (!w) return;
    // acquiring again with a higher priority joins the load and lowers its priority class (manager.acquire)
    land.handles.push(
      this.deps.assets.acquire(
        ChunkType,
        { ns: 'gen', kind: 'collision', folder: w.folder, x: land.x, z: land.z },
        Prio.Critical,
        distance,
        { scoped: true },
      ),
    );
  }

  private drop(land: Land): void {
    for (const h of land.handles) h.release();
    land.handles = [];
    if (land.state === 'queued' || land.state === 'ready') {
      this.unloads += 1;
      this.deps.sim.enqueueLoad({ kind: 'unchunk', lx: land.x, lz: land.z, prio: Prio.Near });
    }
  }

  /** The sim applied (or rejected) a chunk load (`SimBridgeDeps.onLoad`). */
  onSimLoad(
    load: { readonly kind: string; readonly lx?: number; readonly lz?: number },
    error: unknown,
  ): void {
    if (load.kind !== 'chunk' || load.lx === undefined || load.lz === undefined) return;
    const land = this.lands.get(key(load.lx, load.lz));
    if (!land) return;
    if (error) {
      land.state = 'failed';
      land.failedAt = this.deps.now();
      this.deps.log?.warn(
        'world',
        `the sim rejected the collision chunk ${String(load.lx)}-${String(load.lz)}: ${errorText(error)}`,
      );
    } else if (land.state === 'queued' || land.state === 'fetching') {
      land.state = 'ready';
    }
  }

  /** `ChunkMissing{lx, lz}` (31 16.4): the sim needs a chunk now, even outside the window. */
  chunkMissing(lx: number, lz: number): void {
    const w = this.world;
    if (!w || lx < 0 || lz < 0 || lx >= w.landsW || lz >= w.landsH) return;
    const known = this.lands.get(key(lx, lz));
    if (known) {
      if (known.state === 'fetching') this.raise(known, 0);
      else if (known.state === 'failed') {
        this.lands.delete(key(lx, lz));
        this.request(lx, lz, Prio.Critical, 0, false);
      }
      return;
    }
    this.request(lx, lz, Prio.Critical, 0, false);
  }

  /** The collision condition of the loading gate: every land of the window is loaded in the sim. */
  gate(): CollisionGate {
    const w = this.world;
    const c = this.center;
    if (!w || !c) return { ready: 0, wanted: 0 };
    const r = windowRadius(w.mpu);
    let ready = 0;
    let wanted = 0;
    for (let z = c.z - r; z <= c.z + r; z += 1) {
      for (let x = c.x - r; x <= c.x + r; x += 1) {
        if (x < 0 || z < 0 || x >= w.landsW || z >= w.landsH) continue;
        wanted += 1;
        if (this.lands.get(key(x, z))?.state === 'ready') ready += 1;
      }
    }
    return { ready, wanted };
  }

  /** The state of a land (tests, probe), `undefined` when it is not tracked. */
  stateOf(x: number, z: number): ChunkState | undefined {
    return this.lands.get(key(x, z))?.state;
  }

  /** The tracked lands as `"x,z"` (tests). */
  tracked(): string[] {
    return [...this.lands.values()].map((l) => `${String(l.x)},${String(l.z)}`).sort();
  }

  get stats(): StreamerStats {
    let ready = 0;
    let pending = 0;
    let failed = 0;
    for (const l of this.lands.values()) {
      if (l.state === 'ready') ready += 1;
      else if (l.state === 'failed') failed += 1;
      else pending += 1;
    }
    return {
      chunksReady: ready,
      chunksPending: pending,
      chunksFailed: failed,
      requests: this.requests,
      prefetches: this.prefetches,
      unloads: this.unloads,
      retries: this.retries,
      window: this.windowSize,
      retain: this.retainSize,
    };
  }

  dispose(): void {
    this.releaseAll();
    this.world = null;
  }
}

export function createWorldStreamer(deps: StreamerDeps): WorldStreamer {
  return new WorldStreamer(deps);
}
