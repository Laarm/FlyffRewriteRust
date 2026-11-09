// The game data of the players views (Plan/25 section 4.4, 11.2 "reused game modules"): item names, icons and the tooltip composer come
// from the grant UI's data (`grant/data.ts`: items, item-ext and texts bundles, the asset manifest, the wasm rules); this module adds the
// bundles only the players views read (`tables` for the armour set bonus, `skills` for the skill tree and the skill buffs, `quests`) and
// the URL of a game image (the job icons). Everything is loaded on first use, once, and a failed load is retried by the next use.
import { useEffect, useState } from 'react';
import { validateManifest } from '../../../data/bundles';
import { BUNDLES } from '../../../gen/bundles';
import type { BundleDataMap, BundleName } from '../../../gen/bundles';
import type { ClientManifest } from '../../../gen/data/assetManifest';
import { assetUrl, resolveManifest, type ResolvedManifest } from '../../../assets/paths';
import { ensureGrantData, useGrantData } from '../../grant';
import type { GrantData } from '../../grant/data';

let fetcher: typeof fetch | null = null;

/** Tests: replaces `fetch` for the bundles and the asset manifest, and forgets what was loaded (`null` restores). */
export function setDataFetchForTests(replacement: typeof fetch | null): void {
  fetcher = replacement;
  bundles.clear();
  manifest = null;
  assets = null;
}

async function getJson(url: string): Promise<unknown> {
  const res = await (fetcher ?? fetch)(url, { credentials: 'same-origin' });
  if (!res.ok) throw new Error(`${url}: HTTP ${String(res.status)}`);
  return await res.json();
}

// ---- bundles --------------------------------------------------------------------------------------------------------------------

const bundles = new Map<BundleName, Promise<unknown>>();
let manifest: Promise<ReturnType<typeof validateManifest>> | null = null;

/** Loads one bundle (the `data` section of its envelope) by the content-hash URL of the manifest. */
export function loadBundle<N extends BundleName>(name: N): Promise<BundleDataMap[N]> {
  let p = bundles.get(name);
  if (!p) {
    manifest ??= getJson('/data/manifest.json').then(validateManifest);
    p = manifest
      .then(async (m) => {
        const entry = m.bundles[name];
        if (!entry) throw new Error(`bundle "${name}" is not in the manifest`);
        const env = await getJson(entry.url);
        const schema = BUNDLES.find((b) => b.name === name)?.schema;
        if (
          typeof env !== 'object' ||
          env === null ||
          !('data' in env) ||
          !('schema' in env) ||
          env.schema !== schema
        ) {
          throw new Error(`bundle "${name}" has an unexpected format (reload the page)`);
        }
        return env.data;
      })
      .catch((error: unknown) => {
        bundles.delete(name);
        manifest = null;
        throw error;
      });
    bundles.set(name, p);
  }
  return p as Promise<BundleDataMap[N]>;
}

export interface BundleState<N extends BundleName> {
  readonly data: BundleDataMap[N] | null;
  readonly error: string | null;
}

/** A bundle as React state (`data` is `null` while it loads). */
export function useBundle<N extends BundleName>(name: N, enabled = true): BundleState<N> {
  const [state, setState] = useState<BundleState<N>>({ data: null, error: null });
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    loadBundle(name).then(
      (data) => {
        if (live) setState({ data, error: null });
      },
      (error: unknown) => {
        if (live)
          setState({ data: null, error: error instanceof Error ? error.message : String(error) });
      },
    );
    return () => {
      live = false;
    };
  }, [name, enabled]);
  return state;
}

// ---- game images ------------------------------------------------------------------------------------------------------------------

let assets: Promise<ResolvedManifest | null> | null = null;

/** The resolver of game image URLs (`/gen/png/...` of a `.dds`), `null` when the server has no asset manifest. */
export function loadAssets(): Promise<ResolvedManifest | null> {
  assets ??= (async () => {
    try {
      const boot = await getJson('/api/bootstrap');
      const url: unknown =
        typeof boot === 'object' && boot !== null && 'asset_manifest_url' in boot
          ? boot.asset_manifest_url
          : undefined;
      if (typeof url !== 'string') return null;
      // the manifest is typed by the generator that wrote it
      return resolveManifest((await getJson(url)) as ClientManifest);
    } catch {
      return null;
    }
  })();
  return assets;
}

/** The URL of a game image by its resource path (`icon/icon_jobclasstextures.dds`); `null` until known or when unknown. */
export function useAssetUrl(path: string): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void loadAssets().then((resolved) => {
      if (live) setUrl(resolved ? assetUrl(resolved, path) : null);
    });
    return () => {
      live = false;
    };
  }, [path]);
  return url;
}

// ---- the grant UI's data --------------------------------------------------------------------------------------------------------------

export interface PlayerGame {
  /** Items, texts, the compose environment and the wasm rules; `null` while they load. */
  readonly data: GrantData | null;
  readonly error: string | null;
  /** The wasm rules are built: tooltips carry their numbers. */
  readonly rulesReady: boolean;
}

/** The game data of the players views; starts the load on first use. */
export function usePlayerGame(): PlayerGame {
  const data = useGrantData((s) => s.data);
  const error = useGrantData((s) => s.error);
  const rulesPhase = useGrantData((s) => s.rulesPhase);
  useEffect(() => {
    void ensureGrantData();
  }, []);
  return { data, error, rulesReady: rulesPhase === 'ready' };
}

/** `tables.exp.set_item_avail`: the armour set bonus rows; `undefined` while the `tables` bundle loads. */
export function useSetItemAvail(): BundleDataMap['tables']['exp']['set_item_avail'] | undefined {
  return useBundle('tables').data?.exp.set_item_avail;
}
