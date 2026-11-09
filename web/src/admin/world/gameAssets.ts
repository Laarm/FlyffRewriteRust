// What the map takes from the game's data (Plan/25 section 3.5, 11.7): the names of the monsters by prop id (the `movers` bundle) and the
// navigator pictures `ImgNavArrow` and `ImgNavPos` (their PNG forms through the asset manifest). The console is its own entry with no game boot,
// so this is a small loader of its own: `/data/manifest.json` -> the bundle by its content-hash URL, `/api/bootstrap` -> the asset manifest ->
// `assetUrl`. Every step may fail without harming the map: a monster without a name shows `#<prop>` and a missing picture is drawn instead.
import { assetUrl, resolveManifest, type ResolvedManifest } from '../../assets/paths';
import type { ClientManifest } from '../../gen/data/assetManifest';
import type { GamePictures } from './spriteAtlas';

export interface GameAssets {
  /** The name of a monster, NPC or ctrl prop; `undefined` while unknown. */
  readonly moverName: (prop: number) => string | undefined;
  readonly pictures: GamePictures;
}

/** The part of the world meta that names props when the bundle is not available. */
export interface PropNameHints {
  readonly spawns: readonly { readonly prop: number; readonly name: string }[];
  readonly npcs: readonly { readonly prop: number; readonly name: string }[];
}

type FetchLike = (url: string) => Promise<Response>;

export type { FetchLike };

export async function getJson(f: FetchLike, url: string): Promise<unknown> {
  const r = await f(url);
  if (!r.ok) throw new Error(`HTTP ${String(r.status)} for ${url}`);
  return r.json();
}

/** `{ bundles: { name: { url } } }` of `/data/manifest.json`, the URL of one bundle. */
export function bundleUrl(manifest: unknown, name: string): string | null {
  if (typeof manifest !== 'object' || manifest === null) return null;
  const bundles = (manifest as { bundles?: unknown }).bundles;
  if (typeof bundles !== 'object' || bundles === null) return null;
  const entry = (bundles as Record<string, unknown>)[name];
  if (typeof entry !== 'object' || entry === null) return null;
  const url = (entry as { url?: unknown }).url;
  return typeof url === 'string' ? url : null;
}

/** The names of a `movers` bundle envelope: a column table `{ columns, rows }` under `data.movers`. */
export function moverNames(envelope: unknown): Map<number, string> {
  const out = new Map<number, string>();
  const data = (envelope as { data?: { movers?: { columns?: unknown; rows?: unknown } } } | null)
    ?.data;
  const table = data?.movers;
  if (!table || !Array.isArray(table.columns) || !Array.isArray(table.rows)) return out;
  const idAt = (table.columns as unknown[]).indexOf('id');
  const nameAt = (table.columns as unknown[]).indexOf('name');
  if (idAt < 0 || nameAt < 0) return out;
  for (const row of table.rows as unknown[]) {
    if (!Array.isArray(row)) continue;
    const id: unknown = row[idAt];
    const name: unknown = row[nameAt];
    if (typeof id === 'number' && typeof name === 'string' && name !== '') out.set(id, name);
  }
  return out;
}

/** The asset manifest of the server (`/api/bootstrap` names it), resolved for `assetUrl`; loaded once per page. `null` when it is not available. */
let resolver: Promise<ResolvedManifest | null> | null = null;

export function loadResolver(
  f: FetchLike = (u) => fetch(u, { credentials: 'same-origin' }),
): Promise<ResolvedManifest | null> {
  resolver ??= (async () => {
    try {
      const boot = (await getJson(f, '/api/bootstrap')) as { asset_manifest_url?: unknown };
      if (typeof boot.asset_manifest_url !== 'string') return null;
      return resolveManifest((await getJson(f, boot.asset_manifest_url)) as ClientManifest);
    } catch {
      return null;
    }
  })();
  return resolver;
}

/** Tests: forgets the cached manifest. */
export function resetResolver(): void {
  resolver = null;
}

export async function bitmapOf(f: FetchLike, url: string): Promise<ImageBitmap | undefined> {
  try {
    const r = await f(url);
    if (!r.ok) return undefined;
    return await createImageBitmap(await r.blob(), {
      premultiplyAlpha: 'none',
      colorSpaceConversion: 'none',
    });
  } catch {
    return undefined;
  }
}

/** The sprites `[x, y, w, h]` of an atlas of a `ui-atlases` bundle envelope (anything malformed gives an empty list). */
export function atlasFrames(envelope: unknown, inc: string): [number, number, number, number][] {
  const atlas = (envelope as { data?: { atlases?: Record<string, { sprites?: unknown }> } } | null)
    ?.data?.atlases?.[inc];
  if (!atlas || !Array.isArray(atlas.sprites)) return [];
  const out: [number, number, number, number][] = [];
  for (const s of atlas.sprites as unknown[]) {
    const r = s as { x?: unknown; y?: unknown; w?: unknown; h?: unknown } | null;
    if (
      typeof r?.x !== 'number' ||
      typeof r.y !== 'number' ||
      typeof r.w !== 'number' ||
      typeof r.h !== 'number'
    )
      return [];
    out.push([r.x, r.y, r.w, r.h]);
  }
  return out;
}

/** The four frames of the selection pulse from `Theme/NavPosition.inc` (the `ui-atlases` bundle gives the rectangles), or `null`. */
async function navPosFrames(f: FetchLike): Promise<[number, number, number, number][] | null> {
  try {
    const url = bundleUrl(await getJson(f, '/data/manifest.json'), 'ui-atlases');
    if (url === null) return null;
    const frames = atlasFrames(await getJson(f, url), 'theme/navposition.inc');
    return frames.length >= 4 ? frames.slice(0, 4) : null;
  } catch {
    return null;
  }
}

/** Loads what is available; never rejects. */
export async function loadGameAssets(
  hints: PropNameHints | null,
  f: FetchLike = (u) => fetch(u, { credentials: 'same-origin' }),
): Promise<GameAssets> {
  const names = new Map<number, string>();
  for (const s of hints?.spawns ?? []) names.set(s.prop, s.name);
  for (const n of hints?.npcs ?? []) names.set(n.prop, n.name);
  let pictures: GamePictures = {};
  try {
    const manifest = await getJson(f, '/data/manifest.json');
    const url = bundleUrl(manifest, 'movers');
    if (url !== null) {
      try {
        for (const [id, name] of moverNames(await getJson(f, url))) names.set(id, name);
      } catch {
        // the hints of the world meta name the monsters of its spawn regions
      }
    }
  } catch {
    // no bundle manifest: names come from the hints
  }
  const frames = await navPosFrames(f);
  const resolved = await loadResolver(f);
  if (resolved) {
    const [arrow, pos] = await Promise.all(
      ['theme/imgnavarrow.bmp', 'theme/imgnavpos.tga'].map((p) => {
        const u = assetUrl(resolved, p);
        return u === null ? Promise.resolve(undefined) : bitmapOf(f, u);
      }),
    );
    pictures = { arrow, pos, ...(frames ? { posFrames: frames } : {}) };
  }
  return { moverName: (prop) => names.get(prop), pictures };
}
