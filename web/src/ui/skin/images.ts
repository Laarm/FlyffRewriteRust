// UI image loading (Plan/42 §6.1): the cache of decoded theme/icon images with the two colour-key modes, the
// `MISSING` sentinel for textures that do not exist (KQ-30: they render nothing) and an LRU of 64 MB.
//
// URL resolution is the host's (`UiAssetUrls`, the `assetUrl`/`nibbleUrl` of Plan/24 §6.5); decoding is a port
// too (`ImageDecoder`), so the whole module runs in Node tests with synthetic images. The browser decoder is
// {@link createBrowserDecoder}.

import {
  assetUrl,
  cursorHotspot,
  cursorUrl,
  nibbleUrl,
  type ResolvedManifest,
} from '../../assets/paths';
import { isDdsMagic } from './ddsImage';
import { imageBytes, type PixelImage } from './pixels';

/** Colour-key mode of a UI image (Plan/42 §6.1 table). */
export type KeyMode =
  /** `Exact`: exact magenta -> transparent (button sprites, icons, atlases; `CTexture::LoadTexture`). */
  | 'exact'
  /** `Nibble`: the `PaintTexture` key of skin tiles and baked backgrounds (`.nk.png`). */
  | 'nibble';

/** Result of a failed or missing image. */
export const MISSING: unique symbol = Symbol('flyff.ui.missing');
export type Missing = typeof MISSING;

/** The URL resolution of Plan/24 §6.5 for the UI (the host provides it; `null` = not a client-visible file). */
export interface UiAssetUrls {
  /** URL of the `Exact` form of a normalised VFS path (`theme/…`, `icon/…`, `item/…`, `char/…`). */
  assetUrl(vfsPath: string): string | null;
  /** URL of the nibble-keyed `.nk.png` form of a normalised VFS path. */
  nibbleUrl(vfsPath: string): string | null;
  /** URL of a parsed cursor PNG (`/gen/cursor/<fp>/<stem>.png`, Plan/24 §10.3); `null` when it does not exist. */
  cursorUrl?(stem: string): string | null;
  /** `[x, y]` hotspot of a cursor (`manifest.cursors`); `null` when unknown. */
  cursorHotspot?(stem: string): [number, number] | null;
}

/** Decodes the image behind `url` to RGBA bytes; `null` when it does not exist or cannot be decoded. */
export type ImageDecoder = (url: string) => Promise<PixelImage | null>;

export interface UiImageCacheOptions {
  urls: UiAssetUrls;
  decode: ImageDecoder;
  /** LRU capacity in bytes of decoded pixels (default 64 MB, Plan/42 §22). */
  maxBytes?: number;
  /** Receives one message per missing file (dev builds log it; KQ-30). */
  warn?: (message: string) => void;
}

/** A path as the layouts write it: bare file names are under `theme/`. */
export function themePath(file: string): string {
  return normalizePath(file.includes('/') || file.includes('\\') ? file : `theme/${file}`);
}

/** Lower case, `/` separators, no leading `./` (the `normalize_resource_path` rules; lookup is case-insensitive). */
export function normalizePath(path: string): string {
  return path.replace(/\\/g, '/').replace(/^\.\//, '').toLowerCase();
}

interface Entry {
  image: PixelImage | Missing;
  bytes: number;
}

/**
 * Decoded UI images by (path, mode). `get` is the only way in; concurrent requests share one decode.
 * Hits are touched (LRU order), the cache never evicts images that are still being decoded.
 */
export class UiImageCache {
  readonly #urls: UiAssetUrls;
  readonly #decode: ImageDecoder;
  readonly #maxBytes: number;
  readonly #warn: (message: string) => void;
  readonly #entries = new Map<string, Entry>(); // insertion order = LRU order (oldest first)
  readonly #inflight = new Map<string, Promise<PixelImage | Missing>>();
  readonly #warned = new Set<string>();
  #bytes = 0;

  constructor(options: UiImageCacheOptions) {
    this.#urls = options.urls;
    this.#decode = options.decode;
    this.#maxBytes = options.maxBytes ?? 64 * 1024 * 1024;
    this.#warn = options.warn ?? (() => undefined);
  }

  /** Decoded bytes currently held. */
  get bytes(): number {
    return this.#bytes;
  }

  /** The number of cached images (missing markers included). */
  get size(): number {
    return this.#entries.size;
  }

  /** The URL of an image in `mode` for DOM users (`background-image`); `null` for a path that is not client-visible. */
  url(path: string, mode: KeyMode): string | null {
    const norm = normalizePath(path);
    return mode === 'nibble' ? this.#urls.nibbleUrl(norm) : this.#urls.assetUrl(norm);
  }

  /** The cached image, `MISSING`, or `undefined` while it is not loaded (never starts a load). */
  peek(path: string, mode: KeyMode): PixelImage | Missing | undefined {
    return this.#entries.get(key(path, mode))?.image;
  }

  /** Loads (or returns) an image; resolves to `MISSING` for a file that does not exist. */
  get(path: string, mode: KeyMode): Promise<PixelImage | Missing> {
    const k = key(path, mode);
    const hit = this.#entries.get(k);
    if (hit) {
      // touch: move to the young end
      this.#entries.delete(k);
      this.#entries.set(k, hit);
      return Promise.resolve(hit.image);
    }
    const pending = this.#inflight.get(k);
    if (pending) return pending;
    const p = this.#load(k, path, mode);
    this.#inflight.set(k, p);
    return p;
  }

  /** Loads several images; resolves when all settled (missing ones included). */
  async preload(items: readonly { path: string; mode: KeyMode }[]): Promise<void> {
    await Promise.all(items.map((i) => this.get(i.path, i.mode)));
  }

  /** Drops everything (context loss, tests). */
  clear(): void {
    this.#entries.clear();
    this.#bytes = 0;
  }

  async #load(k: string, path: string, mode: KeyMode): Promise<PixelImage | Missing> {
    const norm = normalizePath(path);
    let image: PixelImage | Missing = MISSING;
    try {
      const url = mode === 'nibble' ? this.#urls.nibbleUrl(norm) : this.#urls.assetUrl(norm);
      if (url === null) {
        this.#warnOnce(norm, `UI image ${norm} is not a client-visible file`);
      } else {
        const decoded = await this.#decode(url);
        if (decoded) image = decoded;
        else this.#warnOnce(norm, `UI image ${norm} is missing (${url})`);
      }
    } catch (e) {
      this.#warnOnce(
        norm,
        `UI image ${norm} failed to load: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
    this.#inflight.delete(k);
    const bytes = image === MISSING ? 0 : imageBytes(image);
    this.#entries.set(k, { image, bytes });
    this.#bytes += bytes;
    this.#evict(k);
    return image;
  }

  #warnOnce(path: string, message: string): void {
    if (this.#warned.has(path)) return;
    this.#warned.add(path);
    this.#warn(message);
  }

  #evict(keep: string): void {
    if (this.#bytes <= this.#maxBytes) return;
    for (const [k, e] of this.#entries) {
      if (this.#bytes <= this.#maxBytes) break;
      if (k === keep) continue;
      this.#entries.delete(k);
      this.#bytes -= e.bytes;
    }
  }
}

function key(path: string, mode: KeyMode): string {
  return `${mode}:${normalizePath(path)}`;
}

// ---- URL resolution from the asset manifest (Plan/24 §6.5) -------------------------------------------------

/**
 * `UiAssetUrls` over a resolved `ClientManifest` (`assets/paths.ts`, WP-CLIENT-09): the exact form is the manifest's
 * `assetUrl` (`/gen/png/<fp>/<path>.png` for the theme/icon/item/char partitions) and the nibble form its
 * `nibbleUrl` (`….nk.png`).
 */
export function uiAssetUrlsFromManifest(m: ResolvedManifest): UiAssetUrls {
  return {
    assetUrl: (path) => assetUrl(m, path),
    nibbleUrl: (path) => nibbleUrl(m, path),
    cursorUrl: (stem) => cursorUrl(m.manifest, stem),
    cursorHotspot: (stem) => cursorHotspot(m.manifest, stem),
  };
}

/**
 * `UiAssetUrls` for the dev surfaces without a manifest: any 16-hex fingerprint works against the dev server
 * (a stale one is served with `no-store`, Plan/24 §9.1 step 4).
 */
export function devAssetUrls(fp = '0000000000000000', prefix = '/gen'): UiAssetUrls {
  const png = (path: string, suffix: string): string =>
    `${prefix}/png/${fp}/${path.split('/').map(encodeURIComponent).join('/')}${suffix}`;
  return {
    assetUrl: (p) => png(p, '.png'),
    nibbleUrl: (p) => png(p, '.nk.png'),
    cursorUrl: (stem) => `${prefix}/cursor/${fp}/${encodeURIComponent(stem)}.png`,
  };
}

// ---- the browser decoder -------------------------------------------------------------------------------

/** The options of the browser decoder. */
export interface BrowserDecoderOptions {
  /**
   * Decodes DDS bytes to RGBA (the wasm `dds_bc` job of the decode pool). The server serves the `sfx/` directory as raw DDS, so an atlas
   * there (`sfx/countfight.inc`) has no PNG form: what is fetched is checked for the DDS magic and goes through this function. Without it
   * (or when it answers `null`) a DDS file is a missing image, as before.
   */
  readonly dds?: (bytes: ArrayBuffer) => Promise<PixelImage | null>;
}

/**
 * Fetches a PNG and decodes it to straight-alpha RGBA through `createImageBitmap` + a 2-D canvas read-back
 * (`premultiplyAlpha: 'none'`, no colour-space conversion: the gamma-naive pipeline of Plan/41 R1). A DDS file (the magic `"DDS "`)
 * goes through `options.dds` instead. A 404 (or any non-2xx answer) resolves to `null`.
 */
export function createBrowserDecoder(options: BrowserDecoderOptions = {}): ImageDecoder {
  return async (url) => {
    const res = await fetch(url);
    if (!res.ok) return null;
    const blob = await res.blob();
    if (isDdsMagic(new Uint8Array(await blob.slice(0, 4).arrayBuffer()))) {
      return options.dds ? await options.dds(await blob.arrayBuffer()) : null;
    }
    const bitmap = await createImageBitmap(blob, {
      premultiplyAlpha: 'none',
      colorSpaceConversion: 'none',
    });
    try {
      const px = readPixels(bitmap);
      return px ? { width: px.width, height: px.height, data: px.data, hasAlpha: true } : null;
    } finally {
      bitmap.close();
    }
  };
}

/** The RGBA read-back of a bitmap through a 2-D canvas (`OffscreenCanvas` when the browser has it). */
function readPixels(bitmap: ImageBitmap): ImageData | null {
  const { width, height } = bitmap;
  if (typeof OffscreenCanvas === 'function') {
    const ctx = new OffscreenCanvas(width, height).getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(bitmap, 0, 0);
    return ctx.getImageData(0, 0, width, height);
  }
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(bitmap, 0, 0);
  return ctx.getImageData(0, 0, width, height);
}
