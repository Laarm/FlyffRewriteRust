// The minimap land textures (`CLandscape::m_texMiniMap`, `<land>.dds` of the world folder; ref/99 §4.4, Plan/45 §7.6, 24 §5.2, 40 §9.4;
// WP-CFEAT-28). The server serves the minimaps RAW (they have no PNG form); the browser fetches them through the asset layer and the
// decode pool turns them into RGBA8 (`dds_bc`). `MinimapTiles` holds one decoded tile per land, loads what the painter asks for (a few at
// a time, nearest first through the order of the requests), remembers a missing file (most lands of a world have no minimap) and keeps
// at most `MAX_TILES` decoded tiles (a 256 x 256 RGBA tile is 256 KB).

import { landPath } from '../../render/world/ports';

export const MAX_TILES = 64;
/** At most this many fetch + decode jobs run at once. */
export const MAX_LOADING = 4;

/** `world/<folder>/<folder>XX-ZZ.dds`: the minimap of land `(lx, lz)` (the name pattern of the `.lnd`). */
export function minimapPath(folder: string, lx: number, lz: number): string {
  return landPath(folder, lx, lz).replace(/\.lnd$/, '.dds');
}

/** A decoded tile: RGBA8, straight alpha, row 0 at the top (the DDS order). */
export interface TilePixels {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
}

/** What the tile cache needs from the client: the bytes of a raw file (`null` = not served) and the DDS decoder. */
export interface TileIo {
  fetch(path: string): Promise<ArrayBuffer | null>;
  decode(bytes: ArrayBuffer): Promise<TilePixels | null>;
}

/** The drawable form of a tile (a canvas in the browser; tests pass anything). */
export type TileImage = CanvasImageSource | object;

export interface MinimapTilesOptions {
  readonly io: TileIo;
  /** Turns decoded pixels into something `drawImage` accepts. */
  readonly toImage: (pixels: TilePixels) => TileImage | null;
  /** A tile arrived or failed: the painter redraws. */
  readonly onChange?: () => void;
  readonly maxTiles?: number;
  readonly maxLoading?: number;
}

type Entry =
  | { readonly s: 'loading'; readonly gen: number }
  | { readonly s: 'missing' }
  | { readonly s: 'ready'; readonly image: TileImage; touched: number };

export class MinimapTiles {
  readonly #o: MinimapTilesOptions;
  readonly #entries = new Map<string, Entry>();
  #folder = '';
  #gen = 0;
  #loading = 0;
  #clock = 0;
  /** Counters for the probe and the tests. */
  readonly stats = { requested: 0, loaded: 0, missing: 0, evicted: 0, failed: 0 };

  constructor(options: MinimapTilesOptions) {
    this.#o = options;
  }

  /** The world folder whose minimaps are shown; a different folder drops everything (loads in flight are ignored when they land). */
  setWorld(folder: string): void {
    const f = folder.toLowerCase();
    if (f === this.#folder) return;
    this.#folder = f;
    this.#gen += 1;
    this.#entries.clear();
    this.#loading = 0;
  }

  get folder(): string {
    return this.#folder;
  }

  /** The decoded tile of land `(lx, lz)`, or `null` while it loads or when the world has none; starts the load on first ask. */
  tile(lx: number, lz: number): TileImage | null {
    if (this.#folder === '') return null;
    const key = `${String(lx)},${String(lz)}`;
    const e = this.#entries.get(key);
    if (e) {
      if (e.s === 'ready') {
        this.#clock += 1;
        e.touched = this.#clock;
        return e.image;
      }
      return null;
    }
    if (this.#loading >= (this.#o.maxLoading ?? MAX_LOADING)) return null;
    this.#start(key, lx, lz);
    return null;
  }

  /** `true` while at least one tile is still being fetched or decoded. */
  get busy(): boolean {
    return this.#loading > 0;
  }

  /** The number of decoded tiles held. */
  get size(): number {
    let n = 0;
    for (const e of this.#entries.values()) if (e.s === 'ready') n += 1;
    return n;
  }

  /** Drops every tile (a lost context, the end of the session). */
  clear(): void {
    this.#gen += 1;
    this.#entries.clear();
    this.#loading = 0;
    this.#folder = '';
  }

  #start(key: string, lx: number, lz: number): void {
    const gen = this.#gen;
    this.#entries.set(key, { s: 'loading', gen });
    this.#loading += 1;
    this.stats.requested += 1;
    const path = minimapPath(this.#folder, lx, lz);
    void this.#load(path)
      .then((image) => {
        if (gen !== this.#gen) return; // another world: the result is stale
        this.#loading -= 1;
        if (image === 'missing') {
          this.#entries.set(key, { s: 'missing' });
          this.stats.missing += 1;
        } else if (image === null) {
          this.#entries.set(key, { s: 'missing' });
          this.stats.failed += 1;
        } else {
          this.#clock += 1;
          this.#entries.set(key, { s: 'ready', image, touched: this.#clock });
          this.stats.loaded += 1;
          this.#evict();
        }
        this.#o.onChange?.();
      })
      .catch(() => {
        if (gen !== this.#gen) return;
        this.#loading -= 1;
        this.#entries.set(key, { s: 'missing' });
        this.stats.failed += 1;
        this.#o.onChange?.();
      });
  }

  async #load(path: string): Promise<TileImage | 'missing' | null> {
    const bytes = await this.#o.io.fetch(path);
    if (bytes === null) return 'missing';
    const pixels = await this.#o.io.decode(bytes);
    if (pixels === null) return null;
    return this.#o.toImage(pixels);
  }

  #evict(): void {
    const max = this.#o.maxTiles ?? MAX_TILES;
    if (this.size <= max) return;
    const ready = [...this.#entries.entries()]
      .filter((kv): kv is [string, Entry & { s: 'ready' }] => kv[1].s === 'ready')
      .sort((a, b) => a[1].touched - b[1].touched);
    for (const [key] of ready.slice(0, ready.length - max)) {
      this.#entries.delete(key);
      this.stats.evicted += 1;
    }
  }
}
