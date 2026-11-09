// The item catalogue of the grant UI (Plan/25 section 5.1): the rows of the `items` bundle reduced to what the picker needs, the
// text search (name, or the exact id `#4602`), the kind tree and the facets. Pure functions; `data.ts` feeds them.
import type { ItemProp, SetItemDef } from '../../gen/bundles';
import { II_PET_EGG } from '../../gen/defs/item';
import {
  IK3_EGG,
  IK3_EVENTMAIN,
  IK3_EVENTSUB,
  IK3_VIRTUAL,
  WEAPON_ULTIMATE,
  WEAPON_UNIQUE,
} from '../../gen/defs/itemkind';
import type { KindNode } from '../gen/dto';

/**
 * The icon file of a catalogue row. The seven hatched pet kinds (`IK3_EGG` but not the egg itself) ship their pictures as
 * `<icon>_00`, `_01`, `_02` only, the base file does not exist (bug #017): the catalogue shows grade D, like the game does for an item
 * without a pet record (`game/pets/petData.ts petItemIconFile`, kept apart: the admin console does not import the game).
 */
function catalogIcon(p: Pick<CatalogSource, 'id' | 'icon' | 'item_kind3'>): string {
  if (p.item_kind3 !== IK3_EGG || p.id === II_PET_EGG) return p.icon;
  const dot = p.icon.lastIndexOf('.');
  return dot < 0 ? p.icon : `${p.icon.slice(0, dot)}_00${p.icon.slice(dot)}`;
}

/** The value propItem uses for "no restriction" (`NULL_ID`). */
export const NULL_ID = 0xffffffff;

/** One row of the catalogue. */
export interface CatalogItem {
  readonly id: number;
  readonly name: string;
  /** The icon file below `Item/` (empty = none). */
  readonly icon: string;
  readonly kind1: number;
  readonly kind2: number;
  readonly kind3: number;
  /** `dwLimitLevel1` (the level needed to use it; 0 when there is none). */
  readonly level: number;
  /** `dwItemJob` (`NULL_ID` = every job). */
  readonly job: number;
  /** `dwItemSex` (`NULL_ID` = both). */
  readonly sex: number;
  readonly packMax: number;
  readonly rare: number;
  readonly flag: number;
  readonly charged: boolean;
  /** `IK3_VIRTUAL`: a placeholder that cannot exist in a bag. */
  readonly virtual: boolean;
  /** An ultimate (2) or unique (1) weapon, 0 for the rest. */
  readonly tier: 0 | 1 | 2;
  readonly event: boolean;
  /** Belongs to a named armour set. */
  readonly inSet: boolean;
}

/** `IP_FLAG_BINDS`: soulbound. */
export const FLAG_BINDS = 0x01;

/** The columns of a propItem row the catalogue reads (`ClientItem` and the test rows both qualify). */
export type CatalogSource = Pick<
  ItemProp,
  | 'id'
  | 'name'
  | 'icon'
  | 'item_kind1'
  | 'item_kind2'
  | 'item_kind3'
  | 'limit_level1'
  | 'item_job'
  | 'item_sex'
  | 'pack_max'
  | 'item_rare'
  | 'flag'
  | 'charged'
  | 'refer_stat1'
>;

/** Reduces the bundle rows to catalogue rows; `sets` marks the pieces of the named armour sets. */
export function toCatalog(
  items: readonly CatalogSource[],
  sets: readonly Pick<SetItemDef, 'elems'>[] = [],
): CatalogItem[] {
  const inSet = new Set<number>();
  for (const s of sets) for (const e of s.elems) inSet.add(e.item);
  const out: CatalogItem[] = [];
  for (const p of items) {
    const level = p.limit_level1 === NULL_ID ? 0 : p.limit_level1;
    out.push({
      id: p.id,
      name: p.name,
      icon: catalogIcon(p),
      kind1: p.item_kind1,
      kind2: p.item_kind2,
      kind3: p.item_kind3,
      level,
      job: p.item_job,
      sex: p.item_sex,
      packMax: Math.max(1, p.pack_max),
      rare: p.item_rare,
      flag: p.flag,
      charged: p.charged !== 0,
      virtual: p.item_kind3 === IK3_VIRTUAL,
      tier: p.refer_stat1 === WEAPON_ULTIMATE ? 2 : p.refer_stat1 === WEAPON_UNIQUE ? 1 : 0,
      event: p.item_kind3 === IK3_EVENTMAIN || p.item_kind3 === IK3_EVENTSUB,
      inSet: inSet.has(p.id),
    });
  }
  return out;
}

/** The facets of 25 section 5.1. `null` = no condition. */
export interface CatalogFilter {
  /** Name text, or `#4602` for the exact id. */
  readonly text: string;
  /** A kind-tree key: `''` (all), `1`, `1.2` or `1.2.3`. */
  readonly kind: string;
  readonly minLevel: number | null;
  readonly maxLevel: number | null;
  readonly job: number | null;
  /** 0 male, 1 female. */
  readonly sex: number | null;
  readonly stackable: boolean | null;
  readonly bound: boolean | null;
  readonly inSet: boolean | null;
  /** 0 = no condition, 1 = unique or better, 2 = ultimate. */
  readonly tier: 0 | 1 | 2;
  readonly event: boolean | null;
}

export const NO_FILTER: CatalogFilter = {
  text: '',
  kind: '',
  minLevel: null,
  maxLevel: null,
  job: null,
  sex: null,
  stackable: null,
  bound: null,
  inSet: null,
  tier: 0,
  event: null,
};

/** How well `name` matches `query` (both compared lower case): 0 = not at all; higher is better. */
export function nameScore(name: string, query: string): number {
  const n = name.toLowerCase();
  const q = query.trim().toLowerCase();
  if (q === '') return 1;
  if (n === q) return 100;
  if (n.startsWith(q)) return 80;
  const words = q.split(/\s+/);
  const wordHits = words.filter((w) => n.split(/[\s_-]+/).some((x) => x.startsWith(w))).length;
  if (wordHits === words.length) return 60 + Math.min(10, words.length);
  if (words.every((w) => n.includes(w))) return 40;
  // subsequence: "angsw" finds "Angel Sword"
  const compact = q.replace(/\s+/g, '');
  let at = 0;
  for (const ch of n) if (ch === compact[at]) at += 1;
  return at === compact.length ? 10 : 0;
}

/** The exact id of a `#4602` query, else `null`. */
export function idQuery(text: string): number | null {
  const m = /^\s*#\s*(\d{1,10})\s*$/.exec(text);
  return m?.[1] === undefined ? null : Number(m[1]);
}

function kindMatches(item: CatalogItem, key: string): boolean {
  if (key === '') return true;
  const parts = key.split('.').map(Number);
  const [k1, k2, k3] = parts;
  if (k1 !== undefined && item.kind1 !== k1) return false;
  if (k2 !== undefined && item.kind2 !== k2) return false;
  return k3 === undefined || item.kind3 === k3;
}

/** Applies the filter; with a name query the best matches come first, then by id. */
export function filterCatalog(
  items: readonly CatalogItem[],
  f: CatalogFilter,
  limit = Infinity,
): CatalogItem[] {
  const id = idQuery(f.text);
  const scored: { readonly item: CatalogItem; readonly score: number }[] = [];
  for (const item of items) {
    if (item.virtual) continue;
    if (id !== null) {
      if (item.id !== id) continue;
    } else if (f.text.trim() === '') {
      // everything
    } else if (nameScore(item.name, f.text) === 0) continue;
    if (!kindMatches(item, f.kind)) continue;
    if (f.minLevel !== null && item.level < f.minLevel) continue;
    if (f.maxLevel !== null && item.level > f.maxLevel) continue;
    if (f.job !== null && item.job !== NULL_ID && item.job !== f.job) continue;
    if (f.sex !== null && item.sex !== NULL_ID && item.sex !== f.sex) continue;
    if (f.stackable !== null && item.packMax > 1 !== f.stackable) continue;
    if (f.bound !== null && ((item.flag & FLAG_BINDS) !== 0) !== f.bound) continue;
    if (f.inSet !== null && item.inSet !== f.inSet) continue;
    if (f.tier > 0 && item.tier < f.tier) continue;
    if (f.event !== null && item.event !== f.event) continue;
    scored.push({ item, score: id === null ? nameScore(item.name, f.text) : 1 });
  }
  scored.sort((a, b) => b.score - a.score || a.item.id - b.item.id);
  const out: CatalogItem[] = [];
  for (const s of scored) {
    if (out.length >= limit) break;
    out.push(s.item);
  }
  return out;
}

/** The path of names from the root to the node `key` (`Weapons > Melee weapons > Swords`), or `''`. */
export function kindPath(tree: readonly KindNode[], key: string): string {
  const names: string[] = [];
  let level: readonly KindNode[] = tree;
  const parts = key.split('.');
  for (let i = 0; i < parts.length; i += 1) {
    const want = parts.slice(0, i + 1).join('.');
    const node = level.find((n) => n.key === want);
    if (!node) return '';
    names.push(node.name);
    level = node.children;
  }
  return names.join(' ▸ ');
}
