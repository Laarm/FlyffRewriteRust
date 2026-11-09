// The game data of the grant UI (Plan/25 section 5.1): the `items`, `item-ext` and `texts` bundles the game itself uses, loaded once
// into the admin app; the asset manifest for the icon URLs; and, in a second step that the catalogue does not wait for, the wasm
// rules (`RulesContext` over `tables items item-ext skills`) that give the live tooltip its numbers.
//
// The console is its own entry and has no game boot, so this is a small loader of its own (manifest -> bundle files by content-hash
// URL). The bundles are public to a signed-in console session: the asset cookie of the admin login opens `/data/*` and `/gen/*`.
import { create } from 'zustand';
import { ClientDataIndex, type BundleSource } from '../../data/clientData';
import { validateManifest } from '../../data/bundles';
import { BUNDLES } from '../../gen/bundles';
import type { BundleDataMap, BundleName, ItemExtBundle } from '../../gen/bundles';
import type { ClientManifest } from '../../gen/data/assetManifest';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import {
  assetUrl,
  normalizeResourcePath,
  resolveManifest,
  type ResolvedManifest,
} from '../../assets/paths';
import { parseFeatures } from '../../app/bootstrap';
import { createComposeEnv } from '../../features/tooltip/env';
import type { ComposeEnv } from '../../features/tooltip/model';
import { createRulesContext, type RulesCfgJs, type RulesFacade } from '../../rules';
import { ensureWasm } from '../../wasm/loader';
import type { CatalogMeta, ItemLimits } from '../gen/dto';
import { fetchCatalogMeta } from './api';
import { toCatalog, type CatalogItem } from './catalog';
import { DEFAULT_LIMITS } from './ranges';

type Unsubscribe = () => void;

/** A `BundleSource` over bundles that were parsed by hand. */
export class MapSource implements BundleSource {
  private readonly data = new Map<BundleName, unknown>();
  private readonly listeners = new Set<(name: BundleName) => void>();

  get<N extends BundleName>(name: N): BundleDataMap[N] | undefined {
    return this.data.get(name) as BundleDataMap[N] | undefined;
  }

  has(name: BundleName): boolean {
    return this.data.has(name);
  }

  subscribe(listener: (name: BundleName) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  set<N extends BundleName>(name: N, data: BundleDataMap[N]): void {
    this.data.set(name, data);
    for (const l of [...this.listeners]) l(name);
  }
}

/** Everything the grant UI reads from the game data. */
export interface GrantData {
  readonly catalog: readonly CatalogItem[];
  readonly byId: ReadonlyMap<number, CatalogItem>;
  readonly index: ClientDataIndex;
  readonly env: ComposeEnv;
  readonly features: ClientFeatureConfig;
  /** The URL of an item icon (`Itm_x.dds` of the `icon` column), `null` when the asset manifest has none. */
  readonly iconUrl: (icon: string) => string | null;
  /** The wasm rules once they are loaded (the live tooltip waits for them). */
  readonly rules: () => RulesFacade | null;
  /** The destination parameters of the random-option tables, in the order of the file. */
  readonly optionDsts: readonly number[];
  /** Name of an item (`''` for an unknown id). */
  readonly itemName: (id: number) => string;
}

/** The `RulesContext` switches of the client features (what the game's `rulesConfigOf` passes, plus `only_armor_awakes`). */
export function rulesConfigOf(config: ClientFeatureConfig): RulesCfgJs {
  const f = config.features;
  const g = config.gameplay;
  return {
    bowdmgPropjob: f.bowdmg_propjob,
    wandAutoattacks: f.wand_autoattacks,
    betterArmorBonus: f.better_armor_bonus,
    autoSkillMax: f.auto_skill_max,
    fixGetHpMpFpPoint: f.fix_get_hp_mp_fp_point,
    removeMaxRecoverRate: f.remove_max_recover_rate,
    setMaxSpeed: g.set_max_speed,
    setMaxCast: g.set_max_cast,
    setMaxCrit: g.set_max_crit,
    maxBlockValue: g.max_block_value,
    maxBlockValueGw: g.max_block_value_gw,
    onlyArmorAwakes: f.only_armor_awakes,
  };
}

/** What the loader needs from the browser (tests inject a fake). */
export interface LoaderEnv {
  readonly fetchFn: typeof fetch;
  /** Compiles and instantiates the wasm module; resolves when `createRulesContext` can run. */
  readonly loadWasm: () => Promise<unknown>;
}

const defaultEnv: LoaderEnv = {
  fetchFn: (...a) => fetch(...a),
  loadWasm: () => ensureWasm(),
};

async function getJson(env: LoaderEnv, url: string): Promise<unknown> {
  const res = await env.fetchFn(url, { credentials: 'same-origin' });
  if (!res.ok) throw new Error(`${url}: HTTP ${String(res.status)}`);
  return await res.json();
}

async function getBytes(env: LoaderEnv, url: string): Promise<Uint8Array> {
  const res = await env.fetchFn(url, { credentials: 'same-origin' });
  if (!res.ok) throw new Error(`${url}: HTTP ${String(res.status)}`);
  return new Uint8Array(await res.arrayBuffer());
}

interface Envelope {
  readonly schema: number;
  readonly data: unknown;
}

function envelopeOf(name: string, bytes: Uint8Array): Envelope {
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(bytes));
  } catch (error) {
    throw new Error(`bundle "${name}" is not JSON`, { cause: error });
  }
  if (typeof parsed !== 'object' || parsed === null || !('data' in parsed) || !('schema' in parsed))
    throw new Error(`bundle "${name}" has no envelope`);
  const schema: unknown = parsed.schema;
  const expected = BUNDLES.find((b) => b.name === name)?.schema;
  if (typeof schema !== 'number' || schema !== expected)
    throw new Error(
      `bundle "${name}" has schema ${String(schema)}, this console expects ${String(expected)} (reload the page)`,
    );
  return { schema, data: parsed.data };
}

/** The state kept by the loader between the two steps. */
interface Loaded {
  readonly data: GrantData;
  readonly bundleUrl: (name: BundleName) => string;
  readonly raw: ReadonlyMap<BundleName, Uint8Array>;
  /** Where `loadRules` puts the facade for `GrantData.rules()`. */
  readonly holder: { rules: RulesFacade | null };
}

function randomOptionDsts(ext: ItemExtBundle): number[] {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const g of ext.random_options)
    for (const d of g.dsts)
      if (!seen.has(d.dst)) {
        seen.add(d.dst);
        out.push(d.dst);
      }
  return out;
}

/** Builds the grant data over already parsed bundles (the loader and the tests share it). */
export function buildGrantData(
  source: MapSource,
  features: ClientFeatureConfig,
  resolved: ResolvedManifest | null,
  rules: () => RulesFacade | null = () => null,
  nowSec: () => number = () => Math.floor(Date.now() / 1000),
): GrantData {
  const index = new ClientDataIndex(source);
  const env = createComposeEnv({
    data: () => index,
    features: () => features,
    colorSet: () => 0,
    nowSec,
  });
  const ext = source.get('item-ext');
  const catalog = toCatalog(index.items()?.rows() ?? [], ext?.set_items ?? []);
  const byId = new Map(catalog.map((c) => [c.id, c]));
  const iconCache = new Map<string, string | null>();
  return {
    catalog,
    byId,
    index,
    env,
    features,
    iconUrl: (icon) => {
      if (icon === '' || resolved === null) return null;
      const hit = iconCache.get(icon);
      if (hit !== undefined) return hit;
      const path = normalizeResourcePath(`item/${icon}`);
      const url = path === null ? null : assetUrl(resolved, path);
      iconCache.set(icon, url);
      return url;
    },
    rules,
    optionDsts: ext ? randomOptionDsts(ext) : [],
    itemName: (id) => byId.get(id)?.name ?? '',
  };
}

/** Step 1: the catalogue (three bundles, the asset manifest). */
export async function loadCatalogData(env: LoaderEnv = defaultEnv): Promise<Loaded> {
  const manifest = validateManifest(await getJson(env, '/data/manifest.json'));
  const urlOf = (name: BundleName): string => {
    const entry = manifest.bundles[name];
    if (!entry) throw new Error(`bundle "${name}" is not in the manifest`);
    return entry.url;
  };
  let features: ClientFeatureConfig | null = null;
  let resolved: ResolvedManifest | null = null;
  try {
    const boot = await getJson(env, '/api/bootstrap');
    if (typeof boot === 'object' && boot !== null) {
      features = parseFeatures('features' in boot ? boot.features : undefined).config;
      const am: unknown = 'asset_manifest_url' in boot ? boot.asset_manifest_url : undefined;
      if (typeof am === 'string') {
        // the asset manifest is typed by the generator that wrote it
        resolved = resolveManifest((await getJson(env, am)) as ClientManifest);
      }
    }
  } catch {
    // icons and the feature switches are cosmetic here: the catalogue works without them
  }
  features ??= parseFeatures(undefined).config;
  const names: BundleName[] = ['items', 'item-ext', 'texts'];
  const files = await Promise.all(names.map((n) => getBytes(env, urlOf(n))));
  const source = new MapSource();
  const raw = new Map<BundleName, Uint8Array>();
  names.forEach((n, i) => {
    const bytes = files[i];
    if (!bytes) return;
    raw.set(n, bytes);
    const env_ = envelopeOf(n, bytes);
    // each bundle's data section has the type of its name (checked by the schema number above)
    source.set(n, env_.data as never);
  });
  const holder: { rules: RulesFacade | null } = { rules: null };
  const data = buildGrantData(source, features, resolved, () => holder.rules);
  return { data, bundleUrl: urlOf, raw, holder };
}

/** Step 2: the wasm rules, built from the raw bytes of `tables items item-ext skills`. */
export async function loadRules(
  loaded: Loaded,
  env: LoaderEnv = defaultEnv,
): Promise<RulesFacade | null> {
  await env.loadWasm();
  const need: BundleName[] = ['tables', 'skills'];
  const files = await Promise.all(need.map((n) => getBytes(env, loaded.bundleUrl(n))));
  const [tables, skills] = files;
  const items = loaded.raw.get('items');
  const itemExt = loaded.raw.get('item-ext');
  if (!tables || !skills || !items || !itemExt) return null;
  const facade = createRulesContext(
    { tables, items, itemExt, skills },
    rulesConfigOf(loaded.data.features),
  );
  loaded.holder.rules = facade;
  return facade;
}

// ---- the shared state -----------------------------------------------------------------------------------------------------------

export type DataPhase = 'idle' | 'loading' | 'ready' | 'error';
export type RulesPhase = 'idle' | 'loading' | 'ready' | 'missing' | 'error';

interface DataState {
  readonly phase: DataPhase;
  readonly data: GrantData | null;
  readonly error: string | null;
  readonly rulesPhase: RulesPhase;
  readonly rulesError: string | null;
}

export const useGrantData = create<DataState>(() => ({
  phase: 'idle',
  data: null,
  error: null,
  rulesPhase: 'idle',
  rulesError: null,
}));

let inflight: Promise<void> | null = null;

/** Starts the load (once); the catalogue is usable when `phase` is `ready`, the live tooltip when `rulesPhase` is. */
export function ensureGrantData(env: LoaderEnv = defaultEnv): Promise<void> {
  if (useGrantData.getState().phase === 'ready') return Promise.resolve();
  inflight ??= (async () => {
    useGrantData.setState({ phase: 'loading', error: null });
    let loaded: Loaded;
    try {
      loaded = await loadCatalogData(env);
    } catch (error) {
      useGrantData.setState({
        phase: 'error',
        error: error instanceof Error ? error.message : String(error),
      });
      inflight = null;
      return;
    }
    useGrantData.setState({ phase: 'ready', data: loaded.data, rulesPhase: 'loading' });
    try {
      const facade = await loadRules(loaded, env);
      useGrantData.setState({ rulesPhase: facade ? 'ready' : 'missing' });
    } catch (error) {
      useGrantData.setState({
        rulesPhase: 'error',
        rulesError: error instanceof Error ? error.message : String(error),
      });
    }
  })();
  return inflight;
}

/** Tests: installs ready data (and forgets the loader's state). */
export function setGrantDataForTests(
  data: GrantData | null,
  rulesPhase: RulesPhase = 'ready',
): void {
  inflight = null;
  useGrantData.setState({
    phase: data ? 'ready' : 'idle',
    data,
    error: null,
    rulesPhase: data ? rulesPhase : 'idle',
    rulesError: null,
  });
}

// ---- the server's catalogue metadata ----------------------------------------------------------------------------------------------

interface MetaState {
  readonly meta: CatalogMeta | null;
  readonly error: string | null;
  readonly loading: boolean;
}

/** `GET /admin/api/items/catalog-meta`: the kind tree, the field profiles of every item, the ranges and the switches. */
export const useMeta = create<MetaState>(() => ({ meta: null, error: null, loading: false }));

let metaInflight: Promise<void> | null = null;

/** Loads the metadata once (a failure is retried by the next call). */
export function ensureMeta(): Promise<void> {
  if (useMeta.getState().meta) return Promise.resolve();
  metaInflight ??= (async () => {
    useMeta.setState({ loading: true, error: null });
    try {
      useMeta.setState({ meta: await fetchCatalogMeta(), loading: false });
    } catch (error) {
      useMeta.setState({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      metaInflight = null;
    }
  })();
  return metaInflight;
}

/** Tests: installs the metadata. */
export function setMetaForTests(meta: CatalogMeta | null): void {
  metaInflight = null;
  useMeta.setState({ meta, error: null, loading: false });
}

/** The ranges the form uses: the server's once its metadata arrived, else the defaults it is held to by the golden file. */
export function useLimits(): ItemLimits {
  return useMeta((s) => s.meta?.limits) ?? DEFAULT_LIMITS;
}
