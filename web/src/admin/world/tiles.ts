// The tile logic of the map's base layer (Plan/25 section 3.4): which level to show at a scale, which tiles of that level are in view and
// exist, which coarser tile stands in for one that has not arrived, and the LRU of the loaded tiles with aborted requests. Pure: the GPU
// upload and the fetching are `tileTextures.ts`.
import { TILE_PX, isCovered, texelSize } from '../components/mapGeo';
import type { TilesMeta } from './metaTypes';
import type { Rect } from './view';

export interface TileId {
  /** 0 = the original minimap, 1.. = the overview levels. */
  readonly level: number;
  readonly tx: number;
  readonly tz: number;
}

export const tileKey = (id: TileId): string =>
  `${String(id.level)}/${String(id.tx)}_${String(id.tz)}`;

/** The tiles that exist: level 0 = the covered landscapes; a level-k tile exists when one of its landscapes is covered. */
export class Coverage {
  readonly lands: readonly [number, number];
  readonly maxLevel: number;
  /** Summed-area table, `(lx + 1) * (lz + 1)` cells: `sat[(lz + 1) * (lx_n + 1) + lx + 1]` covered landscapes below-left. */
  readonly #sat: Int32Array;
  readonly #stride: number;

  constructor(tiles: TilesMeta | null, lands: readonly [number, number]) {
    this.lands = lands;
    this.maxLevel = tiles?.max_level ?? 0;
    this.#stride = lands[0] + 1;
    this.#sat = new Int32Array((lands[0] + 1) * (lands[1] + 1));
    if (!tiles) return;
    for (let lz = 0; lz < lands[1]; lz += 1) {
      let row = 0;
      for (let lx = 0; lx < lands[0]; lx += 1) {
        if (isCovered(tiles.covered, lx, lz)) row += 1;
        this.#sat[(lz + 1) * this.#stride + lx + 1] =
          (this.#sat[lz * this.#stride + lx + 1] ?? 0) + row;
      }
    }
  }

  /** Covered landscapes inside lx0..lx1, lz0..lz1 (inclusive, clipped to the world). */
  count(lx0: number, lz0: number, lx1: number, lz1: number): number {
    const x0 = Math.max(0, lx0);
    const z0 = Math.max(0, lz0);
    const x1 = Math.min(this.lands[0] - 1, lx1);
    const z1 = Math.min(this.lands[1] - 1, lz1);
    if (x1 < x0 || z1 < z0) return 0;
    const s = this.#stride;
    const at = (x: number, z: number): number => this.#sat[z * s + x] ?? 0;
    return at(x1 + 1, z1 + 1) - at(x0, z1 + 1) - at(x1 + 1, z0) + at(x0, z0);
  }

  covered(lx: number, lz: number): boolean {
    return this.count(lx, lz, lx, lz) > 0;
  }

  /** `true` when the tile exists on the server. */
  exists(id: TileId): boolean {
    if (id.level < 0 || id.level > this.maxLevel || id.tx < 0 || id.tz < 0) return false;
    const n = 2 ** id.level;
    return this.count(id.tx * n, id.tz * n, id.tx * n + n - 1, id.tz * n + n - 1) > 0;
  }

  /** Whether any landscape is covered at all. */
  get any(): boolean {
    return this.count(0, 0, this.lands[0] - 1, this.lands[1] - 1) > 0;
  }
}

/**
 * The level whose texel size is closest to the current scale `upp` (world units per CSS pixel), ties towards the finer level.
 * Level 0 is the original minimap (`landM / 256` per texel), every level doubles it.
 */
export function chooseLevel(upp: number, landM: number, maxLevel: number): number {
  const t0 = texelSize(landM, 0);
  if (!(upp > t0)) return 0;
  let best = 0;
  let bestErr = Math.abs(upp - t0);
  for (let k = 1; k <= maxLevel; k += 1) {
    const err = Math.abs(upp - t0 * 2 ** k);
    if (err < bestErr) {
      best = k;
      bestErr = err;
    }
  }
  return best;
}

/** The world rectangle `[x0, z0, x1, z1]` of a tile. */
export function tileRect(landM: number, id: TileId): Rect {
  const span = landM * 2 ** id.level;
  return [id.tx * span, id.tz * span, (id.tx + 1) * span, (id.tz + 1) * span];
}

/** The parent tile (one level coarser) or `null` above `maxLevel`. */
export function parentOf(id: TileId, maxLevel: number): TileId | null {
  if (id.level >= maxLevel) return null;
  return { level: id.level + 1, tx: id.tx >> 1, tz: id.tz >> 1 };
}

/** The existing tiles of `level` that overlap `rect`, nearest to the centre of the rectangle first. */
export function tilesIn(rect: Rect, landM: number, level: number, cov: Coverage): TileId[] {
  const span = landM * 2 ** level;
  const nx = Math.ceil(cov.lands[0] / 2 ** level);
  const nz = Math.ceil(cov.lands[1] / 2 ** level);
  const tx0 = Math.max(0, Math.floor(rect[0] / span));
  const tx1 = Math.min(nx - 1, Math.floor(rect[2] / span));
  const tz0 = Math.max(0, Math.floor(rect[1] / span));
  const tz1 = Math.min(nz - 1, Math.floor(rect[3] / span));
  const cx = (rect[0] + rect[2]) / 2;
  const cz = (rect[1] + rect[3]) / 2;
  const out: { id: TileId; d: number }[] = [];
  for (let tz = tz0; tz <= tz1; tz += 1) {
    for (let tx = tx0; tx <= tx1; tx += 1) {
      const id: TileId = { level, tx, tz };
      if (!cov.exists(id)) continue;
      const dx = (tx + 0.5) * span - cx;
      const dz = (tz + 0.5) * span - cz;
      out.push({ id, d: dx * dx + dz * dz });
    }
  }
  out.sort((a, b) => a.d - b.d);
  return out.map((o) => o.id);
}

export interface TilePlan {
  /** What to draw, coarse to fine (a finer tile covers its coarser stand-in). */
  readonly draw: readonly TileId[];
  /** What to load, most urgent first: parents of the missing tiles before the tiles themselves, the centre outwards. */
  readonly wanted: readonly TileId[];
  /** The keys that must not be evicted: the visible tiles of the level, their stand-ins and the loaded ones of the margin. */
  readonly keep: ReadonlySet<string>;
}

/**
 * Plans one frame. `draw` holds, for every visible tile of `level`, the tile itself when `ready(id)` and else its nearest ready ancestor
 * (the blurred placeholder). `wanted` lists the missing ones; `margin` is the same computation for the rectangle grown by one tile
 * (loaded early, only kept and loaded after the visible ones).
 */
export function planTiles(
  view: Rect,
  margin: Rect,
  landM: number,
  level: number,
  cov: Coverage,
  ready: (id: TileId) => boolean,
): TilePlan {
  const visible = tilesIn(view, landM, level, cov);
  const draw = new Map<string, TileId>();
  const wanted: TileId[] = [];
  const queued = new Set<string>();
  const keep = new Set<string>();
  const want = (id: TileId): void => {
    const key = tileKey(id);
    if (queued.has(key) || ready(id)) return;
    queued.add(key);
    wanted.push(id);
  };
  const standIn = (id: TileId): void => {
    // the nearest ancestor that is ready stands in; the parent is requested first, then the rest of the chain of the centre tile
    for (let p = parentOf(id, cov.maxLevel); p !== null; p = parentOf(p, cov.maxLevel)) {
      if (!cov.exists(p)) continue;
      if (ready(p)) {
        draw.set(tileKey(p), p);
        keep.add(tileKey(p));
        return;
      }
    }
  };
  const missing: TileId[] = [];
  for (const id of visible) {
    keep.add(tileKey(id));
    if (ready(id)) {
      draw.set(tileKey(id), id);
    } else {
      missing.push(id);
      standIn(id);
    }
  }
  // parents first
  for (const id of missing) {
    const p = parentOf(id, cov.maxLevel);
    if (p && cov.exists(p)) want(p);
  }
  // the whole chain above the centre tile (cheap: the top levels have a handful of tiles)
  const first = visible[0];
  if (first) {
    for (let p = parentOf(first, cov.maxLevel); p !== null; p = parentOf(p, cov.maxLevel)) {
      if (cov.exists(p)) want(p);
    }
  }
  for (const id of missing) want(id);
  // the margin: loaded after, kept when loaded
  for (const id of tilesIn(margin, landM, level, cov)) {
    keep.add(tileKey(id));
    want(id);
  }
  const ordered = [...draw.values()].sort((a, b) => b.level - a.level);
  return { draw: ordered, wanted, keep };
}

// ---- the cache -------------------------------------------------------------------------------------------------------------------

/** What the cache needs of the tile source. `load` resolves `null` for a tile that does not exist. */
export interface TileCacheOptions<R> {
  readonly capacity: number;
  readonly maxLoading: number;
  load(id: TileId, signal: AbortSignal): Promise<R | null>;
  /** Releases the GPU memory of an evicted tile. */
  dispose(res: R, id: TileId): void;
  /** A tile arrived or failed: the engine redraws. */
  onChange?(): void;
  /** The clock of the retry of failed tiles. */
  now?(): number;
}

type Entry<R> =
  | { readonly s: 'loading'; readonly ctl: AbortController }
  | { readonly s: 'ready'; readonly res: R; touched: number }
  | { readonly s: 'missing' }
  | { readonly s: 'failed'; readonly at: number };

/** A failed tile is requested again after this long. */
export const RETRY_MS = 5_000;

export interface TileStats {
  requested: number;
  loaded: number;
  missing: number;
  failed: number;
  aborted: number;
  evicted: number;
}

/** An LRU of at most `capacity` ready tiles; loads run `maxLoading` at a time and are aborted when their tile leaves the wanted set. */
export class TileCache<R> {
  readonly #o: TileCacheOptions<R>;
  readonly #entries = new Map<string, Entry<R>>();
  readonly #ids = new Map<string, TileId>();
  #loading = 0;
  #clock = 0;
  readonly stats: TileStats = {
    requested: 0,
    loaded: 0,
    missing: 0,
    failed: 0,
    aborted: 0,
    evicted: 0,
  };

  constructor(options: TileCacheOptions<R>) {
    this.#o = options;
  }

  /** Ready tiles held. */
  get size(): number {
    let n = 0;
    for (const e of this.#entries.values()) if (e.s === 'ready') n += 1;
    return n;
  }

  get loading(): number {
    return this.#loading;
  }

  /** The resource of a ready tile (touches it). */
  get(id: TileId): R | undefined {
    const e = this.#entries.get(tileKey(id));
    if (e?.s !== 'ready') return undefined;
    this.#clock += 1;
    e.touched = this.#clock;
    return e.res;
  }

  isReady(id: TileId): boolean {
    return this.#entries.get(tileKey(id))?.s === 'ready';
  }

  state(id: TileId): Entry<R>['s'] | 'none' {
    return this.#entries.get(tileKey(id))?.s ?? 'none';
  }

  /**
   * Starts the loads of `wanted` (in order, up to `maxLoading` at a time), aborts the loads of tiles in neither `wanted` nor `keep`,
   * and evicts down to the capacity (tiles outside `keep` first, the least recently used first).
   */
  request(wanted: readonly TileId[], keep: ReadonlySet<string>): void {
    const want = new Set<string>();
    for (const id of wanted) want.add(tileKey(id));
    for (const [key, e] of this.#entries) {
      if (e.s === 'loading' && !want.has(key) && !keep.has(key)) {
        e.ctl.abort();
        this.#entries.delete(key);
        this.#ids.delete(key);
        this.#loading -= 1;
        this.stats.aborted += 1;
      }
    }
    const now = this.#o.now?.() ?? 0;
    for (const id of wanted) {
      if (this.#loading >= this.#o.maxLoading) break;
      const key = tileKey(id);
      const e = this.#entries.get(key);
      if (e && !(e.s === 'failed' && now - e.at >= RETRY_MS)) continue;
      this.#start(key, id);
    }
    this.#evict(keep);
  }

  #start(key: string, id: TileId): void {
    const ctl = new AbortController();
    this.#entries.set(key, { s: 'loading', ctl });
    this.#ids.set(key, id);
    this.#loading += 1;
    this.stats.requested += 1;
    const settle = (next: Entry<R>): void => {
      const cur = this.#entries.get(key);
      if (cur?.s !== 'loading' || cur.ctl !== ctl) return; // aborted or released meanwhile
      this.#loading -= 1;
      this.#entries.set(key, next);
      this.#o.onChange?.();
    };
    this.#o.load(id, ctl.signal).then(
      (res) => {
        if (ctl.signal.aborted) {
          if (res !== null) this.#o.dispose(res, id);
          return;
        }
        if (res === null) {
          this.stats.missing += 1;
          settle({ s: 'missing' });
        } else {
          this.stats.loaded += 1;
          this.#clock += 1;
          settle({ s: 'ready', res, touched: this.#clock });
        }
      },
      () => {
        if (ctl.signal.aborted) return;
        this.stats.failed += 1;
        settle({ s: 'failed', at: this.#o.now?.() ?? 0 });
      },
    );
  }

  #evict(keep: ReadonlySet<string>): void {
    let n = this.size;
    if (n <= this.#o.capacity) return;
    const ready: { key: string; e: Entry<R> & { s: 'ready' } }[] = [];
    for (const [key, e] of this.#entries) if (e.s === 'ready') ready.push({ key, e });
    // outside the keep set first, then by least recent use
    ready.sort((a, b) => {
      const ka = keep.has(a.key) ? 1 : 0;
      const kb = keep.has(b.key) ? 1 : 0;
      return ka - kb || a.e.touched - b.e.touched;
    });
    for (const { key, e } of ready) {
      if (n <= this.#o.capacity) break;
      const id = this.#ids.get(key);
      if (id) this.#o.dispose(e.res, id);
      this.#entries.delete(key);
      this.#ids.delete(key);
      this.stats.evicted += 1;
      n -= 1;
    }
  }

  /** Disposes every ready tile and aborts every load (the map was hidden for two minutes, or the context was lost). */
  release(): void {
    for (const [key, e] of this.#entries) {
      if (e.s === 'loading') e.ctl.abort();
      else if (e.s === 'ready') {
        const id = this.#ids.get(key);
        if (id) this.#o.dispose(e.res, id);
      }
    }
    this.#entries.clear();
    this.#ids.clear();
    this.#loading = 0;
  }

  /** Forgets the tiles without disposing them (the GL context is gone: the resources died with it). */
  forget(): void {
    for (const e of this.#entries.values()) if (e.s === 'loading') e.ctl.abort();
    this.#entries.clear();
    this.#ids.clear();
    this.#loading = 0;
  }
}

export { TILE_PX };
