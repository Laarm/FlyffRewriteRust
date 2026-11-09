// Hash routes of the console (Plan/25 section 2.3): `#/<category>/<tab>?key=value&...`. The hash keeps the page one document
// (the server serves `/admin` only) and makes every view a shareable deep link. Pure functions; the store is `stores/route.ts`.
//
//   #/dashboard                      the Dashboard (last or first tab)
//   #/players/characters?c=1234      the Characters tab with the character 1234 open in the drawer
//   #/guilds?g=7                     the Guilds category, guild 7 in the drawer
//   #/world/map?w=1&l=0&x=6960&z=3274&zoom=3&sel=p:55    the map (parameters belong to the map view)
//
// Reserved parameters (the shell owns them): `c` character, `a` account, `g` guild, `i` item, `m` monster/object (the record
// open in the drawer) and `pin` (the drawer shown as a full page). Every other key belongs to the view of the tab.

export interface Route {
  readonly category: string;
  /** `null` when the link names none: the shell opens the last remembered tab or the first permitted one. */
  readonly tab: string | null;
  readonly params: Readonly<Record<string, string>>;
}

/** The kinds of record the drawer can show (Plan/25 section 2.1). */
export type RecordKind = 'character' | 'account' | 'guild' | 'item' | 'monster';

export interface RecordRef {
  readonly kind: RecordKind;
  readonly id: string;
}

/** The reserved parameter of each record kind, in the order they are looked up. */
export const RECORD_PARAMS: Readonly<Record<RecordKind, string>> = {
  character: 'c',
  account: 'a',
  guild: 'g',
  item: 'i',
  monster: 'm',
};

const RECORD_ORDER: readonly RecordKind[] = ['character', 'account', 'guild', 'item', 'monster'];

/** Characters the query string leaves readable (`:` in `sel=p:55`, `,` in lists). */
const READABLE = /%3A|%2C/gi;

/** Parses `location.hash` (with or without the `#`). `null` when there is no route (the landing view applies). */
export function parseHash(hash: string): Route | null {
  let text = hash.startsWith('#') ? hash.slice(1) : hash;
  if (!text.startsWith('/')) return null;
  text = text.slice(1);
  const q = text.indexOf('?');
  const path = q < 0 ? text : text.slice(0, q);
  const query = q < 0 ? '' : text.slice(q + 1);
  const [category = '', tab = ''] = path.split('/').map((s) => safeDecode(s));
  if (category === '') return null;
  const params: Record<string, string> = {};
  for (const [k, v] of new URLSearchParams(query)) {
    if (k !== '' && !(k in params)) params[k] = v;
  }
  return { category, tab: tab === '' ? null : tab, params };
}

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** The hash of a route (`#/players/characters?c=12`); parameters sorted by key so equal routes give equal URLs. */
export function formatHash(route: Route): string {
  const path = [route.category, ...(route.tab === null ? [] : [route.tab])]
    .map(encodeURIComponent)
    .join('/');
  const keys = Object.keys(route.params).sort();
  const query =
    keys.length === 0
      ? ''
      : `?${keys.map((k) => `${encodeURIComponent(k)}=${encodeURIComponent(route.params[k] ?? '')}`).join('&')}`;
  return `#/${path}${query.replace(READABLE, (m) => (m.toLowerCase() === '%3a' ? ':' : ','))}`;
}

/** `true` when both routes name the same location. */
export function sameRoute(a: Route | null, b: Route | null): boolean {
  if (a === null || b === null) return a === b;
  return formatHash(a) === formatHash(b);
}

/** The record open in the drawer, when the route has one. */
export function recordOf(route: Route): RecordRef | null {
  for (const kind of RECORD_ORDER) {
    const id = route.params[RECORD_PARAMS[kind]];
    if (id !== undefined && id !== '') return { kind, id };
  }
  return null;
}

/** The route with `record` open in the drawer (any other record is closed; `null` closes the drawer and unpins it). */
export function withRecord(route: Route, record: RecordRef | null): Route {
  const params: Record<string, string> = { ...route.params };
  for (const kind of RECORD_ORDER) Reflect.deleteProperty(params, RECORD_PARAMS[kind]);
  if (record === null) {
    Reflect.deleteProperty(params, 'pin');
  } else {
    params[RECORD_PARAMS[record.kind]] = record.id;
  }
  return { ...route, params };
}

/** The route with the drawer pinned (shown as a full page) or not. */
export function withPinned(route: Route, pinned: boolean): Route {
  const params: Record<string, string> = { ...route.params };
  if (pinned) params['pin'] = '1';
  else Reflect.deleteProperty(params, 'pin');
  return { ...route, params };
}

export function isPinned(route: Route): boolean {
  return route.params['pin'] === '1';
}

/** The route with some parameters set (`undefined` deletes a key). */
export function withParams(
  route: Route,
  changes: Readonly<Record<string, string | undefined>>,
): Route {
  const params: Record<string, string> = { ...route.params };
  for (const [k, v] of Object.entries(changes)) {
    if (v === undefined) Reflect.deleteProperty(params, k);
    else params[k] = v;
  }
  return { ...route, params };
}

/** The record parameters removed: what a tab change keeps of the query (a tab change closes the drawer). */
export function withoutRecord(params: Readonly<Record<string, string>>): Record<string, string> {
  const out: Record<string, string> = { ...params };
  for (const kind of RECORD_ORDER) Reflect.deleteProperty(out, RECORD_PARAMS[kind]);
  Reflect.deleteProperty(out, 'pin');
  return out;
}
