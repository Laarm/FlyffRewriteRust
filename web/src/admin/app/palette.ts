// The command palette's logic (Plan/25 section 2.3): fuzzy matching and ranking of the entries, and the search providers behind
// them. Entries come from three places: the tabs the session may open and the built-in actions (always), the actions the loaded
// categories contribute, and `GET /admin/api/search?q=` (characters, accounts, guilds, items, locations; at most 8 per kind).
// Until WP-ADMIN-17 lands that route answers `needs_feature`: the REST provider then degrades to the STUB provider, which
// finds nothing remote, and the palette still navigates and runs actions.
import { AdminApiError, rest } from '../api/rest';
import type { IconName } from '../components/Icon';

export type PaletteKind =
  'tab' | 'action' | 'character' | 'account' | 'guild' | 'item' | 'location';

/** One row of the palette. */
export interface PaletteItem {
  readonly id: string;
  readonly kind: PaletteKind;
  readonly title: string;
  readonly subtitle?: string;
  /** Extra words the match also covers. */
  readonly keywords?: string;
  readonly icon?: IconName;
  /** An online character ranks before an offline one. */
  readonly online?: boolean;
  run(): void;
}

/** What the server's search answers per hit (the contract is documented in `web/src/admin/NOTES.md`). */
export interface SearchHit {
  readonly kind: 'character' | 'account' | 'guild' | 'item' | 'location';
  readonly id: string;
  readonly title: string;
  readonly subtitle?: string;
  readonly online?: boolean;
  /** Locations: where to centre the map. */
  readonly world?: number;
  readonly x?: number;
  readonly z?: number;
}

/** At most this many hits per kind are shown (and the server sends at most as many). */
export const MAX_PER_KIND = 8;

// ---- matching --------------------------------------------------------------------------------------------------------

function words(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Position-sensitive score of one query token against a lower-case text; `0` = no match. */
function tokenScore(token: string, text: string, ws: readonly string[], fuzzy: boolean): number {
  if (text === token) return 1000;
  if (text.startsWith(token)) return 800;
  const w = ws.findIndex((x) => x.startsWith(token));
  if (w >= 0) return 600 - Math.min(w, 20) * 10;
  const at = text.indexOf(token);
  if (at >= 0) return 400 - Math.min(at, 100);
  if (fuzzy && token.length >= 3) {
    // subsequence (titles only: across a long description nearly anything matches): every character in order, the best alignment
    // wins, and the matched letters must stay close together (a span of at most about 1.5 times the token)
    const limit = token.length + Math.max(2, Math.ceil(token.length / 2));
    let best = Number.POSITIVE_INFINITY;
    for (
      let start = text.indexOf(token[0] ?? '');
      start >= 0;
      start = text.indexOf(token[0] ?? '', start + 1)
    ) {
      let pos = start;
      let ok = true;
      for (let i = 1; i < token.length; i += 1) {
        const next = text.indexOf(token[i] ?? '', pos + 1);
        if (next < 0) {
          ok = false;
          break;
        }
        pos = next;
      }
      if (!ok) break; // a later start cannot match either: the rest of the text only gets shorter
      best = Math.min(best, pos - start + 1);
    }
    if (best <= limit) return Math.max(10, 150 - (best - token.length) * 12);
  }
  return 0;
}

/**
 * How well `query` matches an item (`0` = not at all). Every whitespace-separated token must match the title or the keywords;
 * the title counts double. The score is the sum, so `players char` finds "Players: Characters" before an item that merely
 * contains the letters.
 */
export function scoreItem(
  query: string,
  item: Pick<PaletteItem, 'title' | 'subtitle' | 'keywords' | 'online' | 'kind'>,
): number {
  const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return 1;
  const title = item.title.toLowerCase();
  const titleWords = words(item.title);
  const rest = `${item.subtitle ?? ''} ${item.keywords ?? ''}`.toLowerCase();
  const restWords = words(rest);
  let total = 0;
  for (const t of tokens) {
    const inTitle = tokenScore(t, title, titleWords, true);
    const inRest = tokenScore(t, rest, restWords, false);
    const s = Math.max(inTitle * 2, inRest);
    if (s === 0) return 0;
    total += s;
  }
  // exact title match outranks everything; online characters come before offline ones
  if (title === query.toLowerCase().trim()) total += 2000;
  if (item.online === true) total += 60;
  return total;
}

/** The items that match, best first (ties keep the given order: tabs stay in navigation order). Without a query the order is kept. */
export function rankItems(items: readonly PaletteItem[], query: string): readonly PaletteItem[] {
  if (query.trim() === '') return items;
  return items
    .map((item, i) => ({ item, i, score: scoreItem(query, item) }))
    .filter((x) => x.score > 0)
    .sort((a, b) => b.score - a.score || a.i - b.i)
    .map((x) => x.item);
}

/** The order of the groups on screen. */
export const KIND_ORDER: readonly PaletteKind[] = [
  'tab',
  'action',
  'character',
  'account',
  'guild',
  'item',
  'location',
];

export const KIND_LABEL: Readonly<Record<PaletteKind, string>> = {
  tab: 'Go to',
  action: 'Actions',
  character: 'Characters',
  account: 'Accounts',
  guild: 'Guilds',
  item: 'Items',
  location: 'Places',
};

/** Keeps at most `MAX_PER_KIND` of each kind and orders the kinds as on screen. */
export function groupItems(items: readonly PaletteItem[]): readonly PaletteItem[] {
  const out: PaletteItem[] = [];
  for (const kind of KIND_ORDER) {
    out.push(...items.filter((i) => i.kind === kind).slice(0, MAX_PER_KIND));
  }
  return out;
}

// ---- search providers ------------------------------------------------------------------------------------------------------

/** The result of a provider: hits, or "this kind of search is not available yet" naming the work package. */
export interface SearchResult {
  readonly hits: readonly SearchHit[];
  /** Set when the backend does not exist yet (the palette says so under the list). */
  readonly unavailableWp?: string;
}

export interface SearchProvider {
  search(query: string, signal?: AbortSignal): Promise<SearchResult>;
}

function str(v: unknown): string | null {
  return typeof v === 'string' && v !== '' ? v : typeof v === 'number' ? String(v) : null;
}

function num(v: unknown): number | undefined {
  return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
}

const SECTIONS: readonly { key: string; kind: SearchHit['kind'] }[] = [
  { key: 'characters', kind: 'character' },
  { key: 'accounts', kind: 'account' },
  { key: 'guilds', kind: 'guild' },
  { key: 'items', kind: 'item' },
  { key: 'locations', kind: 'location' },
];

/**
 * Reads the answer of `GET /admin/api/search`: an object with the sections `characters`, `accounts`, `guilds`, `items`, `locations`
 * (each an array; an entry has `id` and `name`, plus `online`, `level`, `job`, `account`, `world`, `x`, `z` where it makes sense), or a flat array
 * of hits `{kind, id, title}`. Anything else is ignored. At most 8 hits per kind are kept.
 */
export function parseSearchResponse(body: unknown): readonly SearchHit[] {
  const hits: SearchHit[] = [];
  const add = (kind: SearchHit['kind'], o: Record<string, unknown>): void => {
    // the players backend answers `{kind, id, label, detail, online}` (WP-ADMIN-17); the contract of NOTES.md used name/title/subtitle
    const title = str(o['name']) ?? str(o['title']) ?? str(o['label']);
    const id = kind === 'account' ? (str(o['name']) ?? str(o['id'])) : str(o['id']);
    if (title === null || id === null) return;
    const bits: string[] = [];
    if (num(o['level']) !== undefined) bits.push(`Lv ${String(o['level'])}`);
    const job = str(o['job']);
    if (job) bits.push(job);
    const account = str(o['account']);
    if (account && kind === 'character') bits.push(`account ${account}`);
    const sub =
      str(o['subtitle']) ?? str(o['detail']) ?? (bits.length > 0 ? bits.join(', ') : null);
    const world = num(o['world']);
    const x = num(o['x']);
    const z = num(o['z']);
    hits.push({
      kind,
      id,
      title,
      ...(sub ? { subtitle: sub } : {}),
      ...(o['online'] === true ? { online: true } : {}),
      ...(world !== undefined ? { world } : {}),
      ...(x !== undefined ? { x } : {}),
      ...(z !== undefined ? { z } : {}),
    });
  };
  if (Array.isArray(body)) {
    for (const e of body) {
      if (typeof e !== 'object' || e === null) continue;
      const o = e as Record<string, unknown>;
      const kind = SECTIONS.find((s) => s.kind === o['kind'])?.kind;
      if (kind) add(kind, o);
    }
  } else if (typeof body === 'object' && body !== null) {
    const o = body as Record<string, unknown>;
    for (const { key, kind } of SECTIONS) {
      const list = o[key];
      if (!Array.isArray(list)) continue;
      for (const e of list.slice(0, MAX_PER_KIND)) {
        if (typeof e === 'object' && e !== null) add(kind, e as Record<string, unknown>);
      }
    }
  }
  return hits;
}

/** `GET /admin/api/search?q=` (WP-ADMIN-17). */
export const restSearchProvider: SearchProvider = {
  async search(query, signal) {
    const body = await rest.get<unknown>('/admin/api/search', {
      query: { q: query },
      ...(signal ? { signal } : {}),
    });
    return { hits: parseSearchResponse(body) };
  },
};

/** The stand-in until the search route exists: nothing remote to find, and it says what is missing. */
export function stubSearchProvider(wp = 'WP-ADMIN-17'): SearchProvider {
  return {
    search() {
      return Promise.resolve({ hits: [], unavailableWp: wp });
    },
  };
}

/**
 * The provider the palette uses: `primary` until it answers `needs_feature` (or 403: the session may not search), then the stub for the
 * rest of the page's life, so a missing backend costs one request, not one per keystroke.
 */
export function degradeToStub(primary: SearchProvider): SearchProvider {
  let stub: SearchProvider | null = null;
  return {
    async search(query, signal) {
      if (stub) return await stub.search(query, signal);
      try {
        return await primary.search(query, signal);
      } catch (error) {
        if (
          error instanceof AdminApiError &&
          (error.code === 'needs_feature' || error.code === 'forbidden')
        ) {
          stub = stubSearchProvider(error.missingWp ?? 'WP-ADMIN-17');
          return await stub.search(query, signal);
        }
        throw error;
      }
    },
  };
}
