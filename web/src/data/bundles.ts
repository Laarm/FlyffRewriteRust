// The bundle loader (Plan/40 section 3.4, Plan/14 section 10, Plan/24 section 13, WP-CLIENT-08).
//
//   manifest (`/data/manifest.json`, no-cache) -> boot / lazy classes -> bundles by content-hash URL
//
// * pre-login subset = the public bundles of 24 section 13 (`texts resdata-layouts ui-atlases sounds music names`),
//   fetched at priority Critical; when the server is not gated the rest of the boot set follows at Near (fetched
//   while the login screen is shown); when `gated` only `onLoggedIn()` loads it (the asset cookie exists then);
// * lazy bundles (`help quests defines`) load on `ensure(name)` (de-duplicated);
// * JSON parsing runs in the worker pool (`json` job: the envelope comes back by structured clone); without a pool
//   (dev surfaces, early tests) the main thread parses;
// * the main thread keeps the raw bytes of exactly `tables items item-ext skills` (the input of
//   `RulesContext::new`, 16 section 18.4; the worker gets a copy); `motions` and `model-bounds` are TS lookups only;
// * `schema` of the manifest entry and of the envelope must equal the generated `BUNDLES[name].schema`; a mismatch is
//   impossible with one executable (14 section 10.8) and means a stale cache: `onSchemaMismatch` (`reloadOnce('data')`).

import { BUNDLES, BUNDLE_FORMAT } from '../gen/bundles';
import type { BundleDataMap, BundleManifest, BundleName } from '../gen/bundles';
import type { AssetSource } from '../assets/source';
import type { Unsubscribe } from '../platform/visibility';
import { Prio } from '../workers/priority';

/** The bundles the server serves before login when it is gated (Plan/24 section 13). */
export const PUBLIC_BUNDLES: readonly BundleName[] = [
  'texts',
  'resdata-layouts',
  'ui-atlases',
  'sounds',
  'music',
  'names',
];

/** The bundles whose raw bytes the main thread keeps for the wasm `RulesContext` (Plan/40 section 3.4 rule 3). */
export const RETAINED_BUNDLES: readonly BundleName[] = ['tables', 'items', 'item-ext', 'skills'];

/** The parse of a bundle file: the worker `json` job, or `JSON.parse` on the main thread. */
export interface BundleEnvelopeLike {
  readonly bundle: string;
  readonly schema: number;
  readonly data: unknown;
}

export type BundleParser = (bytes: ArrayBuffer, prio: Prio) => Promise<BundleEnvelopeLike>;

/** A parser that submits `json` jobs to `pool` (`WorkerPool.submit`). */
export function workerBundleParser(pool: {
  submit(spec: { kind: 'json'; bytes: ArrayBuffer; prio: Prio }): {
    promise: Promise<BundleEnvelopeLike>;
  };
}): BundleParser {
  return async (bytes, prio) => {
    const r = await pool.submit({ kind: 'json', bytes, prio }).promise;
    return { bundle: r.bundle, schema: r.schema, data: r.data };
  };
}

/** The fallback parser (no worker pool). */
export const mainThreadBundleParser: BundleParser = (bytes) => {
  try {
    const parsed = JSON.parse(new TextDecoder().decode(bytes)) as BundleEnvelopeLike;
    return Promise.resolve(parsed);
  } catch (error) {
    return Promise.reject(error instanceof Error ? error : new Error(String(error)));
  }
};

export class BundleSchemaError extends Error {
  constructor(
    readonly bundle: string,
    readonly expected: number,
    readonly got: number | string,
  ) {
    super(`bundle "${bundle}" has schema ${String(got)}, this client expects ${String(expected)}`);
    this.name = 'BundleSchemaError';
  }
}

export class BundleLoadError extends Error {
  constructor(
    readonly bundle: string,
    message: string,
    options?: ErrorOptions,
  ) {
    super(`bundle "${bundle}": ${message}`, options);
    this.name = 'BundleLoadError';
  }
}

export interface BundleLoaderOptions {
  /** Serves `{ ns: 'data', bundle }` keys (the `HttpAssetSource` with the bundle manifest set). */
  readonly source: Pick<AssetSource, 'fetch'>;
  /** Fetches and parses the bundle manifest (default: `fetch(url, { cache: 'no-cache' })`). */
  readonly fetchManifest?: (url: string) => Promise<unknown>;
  readonly parse?: BundleParser;
  /** `bootstrap.gated`: the non-public bundles wait for `onLoggedIn()`. */
  readonly gated?: boolean | (() => boolean);
  /** A schema mismatch: the boot calls `reloadOnce('data')`. */
  readonly onSchemaMismatch?: (error: BundleSchemaError) => void;
  /** The manifest is in (before any bundle is fetched): the boot sets the source's bundle manifest here. */
  readonly onManifest?: (manifest: BundleManifest) => void;
  readonly now?: () => number;
  readonly log?: {
    info(scope: string, message: string): void;
    warn(scope: string, message: string): void;
  };
}

export interface BundleStat {
  readonly bytes: number;
  /** Fetch + parse (ms, main-thread clock). */
  readonly ms: number;
}

const NAMES = new Set<string>(BUNDLES.map((b) => b.name));
const SCHEMA = new Map<string, number>(BUNDLES.map((b) => [b.name, b.schema]));

export function isBundleName(name: string): name is BundleName {
  return NAMES.has(name);
}

export class BundleLoader {
  private manifestValue: BundleManifest | null = null;
  private readonly data = new Map<BundleName, unknown>();
  private readonly rawBytes = new Map<BundleName, ArrayBuffer>();
  private readonly inflight = new Map<BundleName, Promise<void>>();
  private readonly listeners = new Set<(name: BundleName) => void>();
  private readonly statsMap = new Map<BundleName, BundleStat>();
  private loggedIn = false;
  private bootPromise: Promise<void> | null = null;
  private readonly parse: BundleParser;
  private readonly now: () => number;

  constructor(private readonly o: BundleLoaderOptions) {
    this.parse = o.parse ?? mainThreadBundleParser;
    this.now = o.now ?? (() => performance.now());
  }

  /** The loaded bundle manifest (`null` before `loadManifest`). */
  get manifest(): BundleManifest | null {
    return this.manifestValue;
  }

  /** `manifest.data_version` (`''` before the manifest is loaded). */
  get dataVersion(): string {
    return this.manifestValue?.data_version ?? '';
  }

  get gated(): boolean {
    const g = this.o.gated;
    return typeof g === 'function' ? g() : g === true;
  }

  /** `true` once `onLoggedIn()` ran. */
  get isLoggedIn(): boolean {
    return this.loggedIn;
  }

  // ---- manifest -----------------------------------------------------------------------------------------------

  /** Fetches `/data/manifest.json` (no-cache) and validates its shape. */
  async loadManifest(url = '/data/manifest.json'): Promise<BundleManifest> {
    const fetchManifest = this.o.fetchManifest ?? defaultFetchManifest;
    const raw = await fetchManifest(url);
    const m = validateManifest(raw);
    this.manifestValue = m;
    this.o.onManifest?.(m);
    return m;
  }

  /** The bundle names of a load class present in the manifest, in catalogue order. */
  namesOf(load: 'boot' | 'lazy'): BundleName[] {
    const m = this.manifestValue;
    if (!m) return [];
    return BUNDLES.filter((b) => m.bundles[b.name]?.load === load).map((b) => b.name);
  }

  /** The pre-login subset: the public bundles that the manifest has. */
  preLoginSet(): BundleName[] {
    const m = this.manifestValue;
    return PUBLIC_BUNDLES.filter((n) => m?.bundles[n] !== undefined);
  }

  // ---- loading ------------------------------------------------------------------------------------------------

  /** Loads the pre-login subset at priority Critical. */
  async loadPreLogin(): Promise<void> {
    await Promise.all(this.preLoginSet().map((n) => this.fetchBundle(n, Prio.Critical)));
  }

  /**
   * Loads the rest of the boot set at priority Near. While `gated` and not logged in this loads nothing (the
   * gated bundles need the asset cookie): `onLoggedIn()` does it then.
   */
  loadBootSet(): Promise<void> {
    if (this.gated && !this.loggedIn) return Promise.resolve();
    this.bootPromise ??= Promise.all(
      this.namesOf('boot').map((n) => this.fetchBundle(n, Prio.Near)),
    ).then(() => undefined);
    return this.bootPromise;
  }

  /** `/api/login` succeeded: the asset cookie is set, a gated server's boot set can load now. */
  onLoggedIn(): Promise<void> {
    this.loggedIn = true;
    return this.gated ? this.loadBootSet() : (this.bootPromise ?? Promise.resolve());
  }

  /** Resolves when every boot bundle is in (loads it when it was not started, subject to the gate). */
  async whenBootSetLoaded(): Promise<void> {
    await this.loadBootSet();
  }

  /** A lazy bundle (`help`, `quests`, `defines`), or any bundle not loaded yet. De-duplicated. */
  async ensure<N extends BundleName>(
    name: N,
    prio: Prio = Prio.Prefetch,
  ): Promise<BundleDataMap[N]> {
    await this.fetchBundle(name, prio);
    const data = this.data.get(name);
    if (data === undefined) throw new BundleLoadError(name, 'not available');
    return data as BundleDataMap[N];
  }

  has(name: BundleName): boolean {
    return this.data.has(name);
  }

  get<N extends BundleName>(name: N): BundleDataMap[N] | undefined {
    return this.data.get(name) as BundleDataMap[N] | undefined;
  }

  /** The original bytes of `tables`, `items`, `item-ext` or `skills` (`undefined` for every other bundle). */
  raw(name: BundleName): ArrayBuffer | undefined {
    return this.rawBytes.get(name);
  }

  /** Names of the loaded bundles. */
  get loaded(): ReadonlySet<BundleName> {
    return new Set(this.data.keys());
  }

  /** Size and time of every loaded bundle (the probe, the perf overlay). */
  get stats(): ReadonlyMap<BundleName, BundleStat> {
    return this.statsMap;
  }

  /** Calls `listener` after each bundle was added. */
  subscribe(listener: (name: BundleName) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private fetchBundle(name: BundleName, prio: Prio): Promise<void> {
    if (this.data.has(name)) return Promise.resolve();
    const running = this.inflight.get(name);
    if (running) return running;
    const p = this.load(name, prio).finally(() => {
      this.inflight.delete(name);
    });
    this.inflight.set(name, p);
    return p;
  }

  private async load(name: BundleName, prio: Prio): Promise<void> {
    const entry = this.manifestValue?.bundles[name];
    if (!entry) throw new BundleLoadError(name, 'the bundle manifest does not list it');
    const expected = SCHEMA.get(name) ?? 0;
    if (entry.schema !== expected)
      this.mismatch(new BundleSchemaError(name, expected, entry.schema));
    const t0 = this.now();
    let bytes: ArrayBuffer;
    try {
      bytes = await this.o.source.fetch(
        { ns: 'data', bundle: name },
        prio,
        new AbortController().signal,
      );
    } catch (error) {
      throw new BundleLoadError(name, `fetch failed: ${errorText(error)}`, { cause: error });
    }
    const byteLength = bytes.byteLength;
    // raw bytes stay here for wasm; the worker gets a copy
    const retained = RETAINED_BUNDLES.includes(name);
    const toWorker = retained ? bytes.slice(0) : bytes;
    let envelope: BundleEnvelopeLike;
    try {
      envelope = await this.parse(toWorker, prio);
    } catch (error) {
      throw new BundleLoadError(name, `parse failed: ${errorText(error)}`, { cause: error });
    }
    if (envelope.bundle !== name) {
      throw new BundleLoadError(name, `the file is the bundle "${envelope.bundle}"`);
    }
    if (envelope.schema !== expected)
      this.mismatch(new BundleSchemaError(name, expected, envelope.schema));
    if (retained) this.rawBytes.set(name, bytes);
    this.data.set(name, envelope.data);
    this.statsMap.set(name, { bytes: byteLength, ms: this.now() - t0 });
    for (const listener of [...this.listeners]) {
      try {
        listener(name);
      } catch (error) {
        this.o.log?.warn('data', `bundle listener failed: ${errorText(error)}`);
      }
    }
  }

  private mismatch(error: BundleSchemaError): never {
    this.o.onSchemaMismatch?.(error);
    throw error;
  }
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** `GET <url>` with `cache: 'no-cache'` over `fetchFn` (the bundle manifest is revalidated on every load, 14 section 10.3). */
export function manifestFetcher(fetchFn: typeof fetch): (url: string) => Promise<unknown> {
  return async (url) => {
    const res = await fetchFn(url, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`HTTP ${String(res.status)}`);
    return (await res.json()) as unknown;
  };
}

const defaultFetchManifest = (url: string): Promise<unknown> => manifestFetcher(fetch)(url);

/** Checks the shape of `/data/manifest.json` (14 section 10.3); throws a descriptive error. */
export function validateManifest(raw: unknown): BundleManifest {
  if (typeof raw !== 'object' || raw === null) throw new Error('bundle manifest is not an object');
  const m = raw as Partial<BundleManifest>;
  if (m.format !== BUNDLE_FORMAT) {
    throw new BundleSchemaError('manifest', BUNDLE_FORMAT, String(m.format));
  }
  if (typeof m.data_version !== 'string' || m.data_version === '') {
    throw new Error('bundle manifest has no data_version');
  }
  const bundles: unknown = (raw as { bundles?: unknown }).bundles;
  if (typeof bundles !== 'object' || bundles === null) {
    throw new Error('bundle manifest has no bundles');
  }
  return m as BundleManifest;
}

/**
 * Time (ms) of the structured-clone round trip of `value`: what the main thread pays to receive a parsed
 * bundle from the worker (the deserialisation half is on the main thread). Plan/40 section 3.4 rule 4: items <= 40 ms.
 */
export function measureCloneMs(
  value: unknown,
  now: () => number = () => performance.now(),
): number {
  const t0 = now();
  structuredClone(value);
  return now() - t0;
}
