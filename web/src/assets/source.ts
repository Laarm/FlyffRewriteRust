// Asset keys and sources (Plan/40 section 9.1, WP-CLIENT-09). An `AssetKey` names one thing the client loads; an
// `AssetSource` turns a key into bytes. `HttpAssetSource` resolves URLs over the two manifests (24 section 6.5) and
// fetches them through the `FetchScheduler`; `FakeAssetSource` (test/fakes) serves an in-memory map; the pack source
// of M8 (WP-CLIENT-33) wraps `HttpAssetSource`.

import type { BundleManifest } from '../gen/bundles';
import type { ClientManifest } from '../gen/data/assetManifest';
import type { Prio } from '../workers/priority';
import { AssetError } from './errors';
import type { FetchScheduler } from './fetchScheduler';
import {
  allBinUrl,
  assetUrl,
  collisionUrl,
  cursorHotspot,
  cursorUrl,
  nibbleUrl,
  normalizeResourcePath,
  resolveManifest,
  type ResolvedManifest,
} from './paths';

export type AssetKey =
  /** A raw `Resource/` file by its normalised VFS path (24 section 5.1). */
  | { ns: 'res'; path: string }
  /** A collision chunk of a land: `/gen/collision/<fp>/<folder>/<x>-<z>.bin` (every land of a world has one). */
  | { ns: 'gen'; kind: 'collision'; folder: string; x: number; z: number }
  /** `FLYK` of all ctrl colliders (24 section 10.8) -> `SimWorld.load_ctrl_models`. */
  | { ns: 'gen'; kind: 'ctrl-models' }
  /** `FLMT` motion table (31 section 12.1) -> `SimWorld.load_motion_table`. */
  | { ns: 'gen'; kind: 'motion-table' }
  /** `FLMB` model bounds (31 section 11.5) -> `SimWorld.load_model_bounds`. */
  | { ns: 'gen'; kind: 'model-bounds' }
  /** A parsed cursor as PNG (the hotspot is `HttpAssetSource.cursorHotspot`). */
  | { ns: 'gen'; kind: 'cursor'; stem: string }
  /** A nibble-keyed skin image (42's skin bake). */
  | { ns: 'gen'; kind: 'png-nk'; path: string }
  /** A data bundle by name (the URL comes from the bundle manifest). */
  | { ns: 'data'; bundle: string };

/** `res:model/mvr_male.chr`, `gen:collision:wdmadrigal/13-6`, `gen:motion-table`, `data:items`. */
export function keyString(k: AssetKey): string {
  switch (k.ns) {
    case 'res':
      return `res:${k.path}`;
    case 'data':
      return `data:${k.bundle}`;
    case 'gen':
      switch (k.kind) {
        case 'collision':
          return `gen:collision:${k.folder}/${String(k.x)}-${String(k.z)}`;
        case 'cursor':
          return `gen:cursor:${k.stem}`;
        case 'png-nk':
          return `gen:png-nk:${k.path}`;
        case 'ctrl-models':
        case 'motion-table':
        case 'model-bounds':
          return `gen:${k.kind}`;
      }
  }
}

/** A `res` key from a path that may not be normalised (a bundle value, user input); `null` for an invalid path. */
export function resKey(path: string): AssetKey | null {
  const p = normalizeResourcePath(path);
  return p === null ? null : { ns: 'res', path: p };
}

/** Request attributes the source passes to its scheduler (an in-memory source ignores them). */
export interface FetchMeta {
  /** Tie-break inside a priority class: lower first. */
  readonly distance?: number;
  /** World generation; stale queued requests are cancelled (Plan/40 section 8.4). */
  readonly generation?: number | null;
}

export interface AssetSource {
  /** The URL of a key, `null` = not client-visible or unknown (before its manifest is loaded). */
  url(k: AssetKey): string | null;
  /** Rejects with `AssetError`. */
  fetch(k: AssetKey, prio: Prio, signal: AbortSignal, meta?: FetchMeta): Promise<ArrayBuffer>;
  /** The requester's distance changed: re-ranks a queued request. */
  setDistance?(k: AssetKey, distance: number): void;
  /** The world generation was bumped: queued requests of older generations are cancelled. */
  setGeneration?(generation: number): void;
}

export interface HttpAssetSourceOptions {
  readonly scheduler: FetchScheduler;
  readonly manifest?: ClientManifest | null;
  readonly bundles?: BundleManifest | null;
}

/** The production source: URL resolution of Plan/24 section 6.5 plus the fetch scheduler. */
export class HttpAssetSource implements AssetSource {
  readonly scheduler: FetchScheduler;
  private resolved: ResolvedManifest | null = null;
  private bundleManifest: BundleManifest | null;

  constructor(options: HttpAssetSourceOptions) {
    this.scheduler = options.scheduler;
    if (options.manifest) this.resolved = resolveManifest(options.manifest);
    this.bundleManifest = options.bundles ?? null;
  }

  /** The client asset manifest (`/data/asset-manifest.<fp>.json`), `null` before it is loaded. */
  get manifest(): ClientManifest | null {
    return this.resolved?.manifest ?? null;
  }

  setAssetManifest(manifest: ClientManifest | null): void {
    this.resolved = manifest ? resolveManifest(manifest) : null;
  }

  /** The bundle manifest (`/data/manifest.json`): the URLs of `{ ns: 'data' }` keys. */
  setBundleManifest(manifest: BundleManifest | null): void {
    this.bundleManifest = manifest;
  }

  url(k: AssetKey): string | null {
    if (k.ns === 'data') return this.bundleManifest?.bundles[k.bundle]?.url ?? null;
    const m = this.resolved;
    if (!m) return null;
    switch (k.ns) {
      case 'res':
        return assetUrl(m, k.path);
      case 'gen':
        switch (k.kind) {
          case 'collision':
            return collisionUrl(m.manifest, k.folder, k.x, k.z);
          case 'ctrl-models':
          case 'motion-table':
          case 'model-bounds':
            return allBinUrl(m.manifest, k.kind);
          case 'cursor':
            return cursorUrl(m.manifest, k.stem);
          case 'png-nk':
            return nibbleUrl(m, k.path);
        }
    }
  }

  /** `[x, y]` hotspot of a parsed cursor (`CSS cursor: url(...) x y, auto`). */
  cursorHotspot(stem: string): [number, number] | null {
    return this.resolved ? cursorHotspot(this.resolved.manifest, stem) : null;
  }

  fetch(k: AssetKey, prio: Prio, signal: AbortSignal, meta: FetchMeta = {}): Promise<ArrayBuffer> {
    const key = keyString(k);
    const url = this.url(k);
    if (url === null) {
      return Promise.reject(new AssetError('notFound', key, 'not a client-visible asset'));
    }
    return this.scheduler.fetch({
      key,
      url,
      prio,
      signal,
      ...(meta.distance === undefined ? {} : { distance: meta.distance }),
      ...(meta.generation === undefined ? {} : { generation: meta.generation }),
    });
  }

  setDistance(k: AssetKey, distance: number): void {
    this.scheduler.setDistance(keyString(k), distance);
  }

  setGeneration(generation: number): void {
    this.scheduler.setGeneration(generation);
  }
}
