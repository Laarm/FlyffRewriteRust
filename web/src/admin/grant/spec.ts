// The item being built: the `ItemSpec` of the wire (25 section 11.10), which fields the builder shows for an item (the server's
// profile of the item, `CatalogMeta.profiles`), the cleaning of a spec to those fields and the one-line description of a basket line.
import type {
  CatalogMeta,
  Element,
  ItemFieldsDto,
  ItemLimits,
  ItemSpec,
  KeepUntil,
  PetRecord,
  Recipients,
  VisSpec,
} from '../gen/dto';
import { ELEMENT_KEYS, ELEMENTS, PET_LEVELS } from './ranges';

/** A fresh spec: `count` pieces of the item, no extras. */
export function blankSpec(itemId: number, count = 1): ItemSpec {
  return {
    item_id: itemId,
    count,
    upgrade: 0,
    element: null,
    element_level: 0,
    random_options: [],
    safe: false,
    piercing: [],
    ultimate_gems: [],
    vis: [],
    pet: null,
    charged: false,
    keep_until: 'item',
    bound: null,
    text: null,
    guild_id: null,
  };
}

/** What the form shows for an item. */
export interface Visibility {
  readonly count: boolean;
  readonly upgrade: boolean;
  readonly element: boolean;
  readonly options: boolean;
  readonly piercing: boolean;
  readonly ultimate: boolean;
  readonly vis: boolean;
  readonly pet: boolean;
  readonly charged: boolean;
  /** Expiry and the bound flag apply to every item. */
  readonly expiry: boolean;
  readonly bound: boolean;
  readonly text: boolean;
  readonly guild: boolean;
}

/** The fields of an item by its profile (`null` = unknown item: only the count, expiry and bound flag). */
export function visibilityOf(fields: ItemFieldsDto | null): Visibility {
  return {
    count: fields === null || fields.stack_max > 1,
    upgrade: (fields?.upgrade_max ?? 0) > 0,
    element: fields?.element === true,
    options: fields?.random_options === true,
    piercing: (fields?.piercing_max ?? 0) > 0,
    ultimate: fields?.ultimate === true,
    vis: fields?.vis === true,
    pet: fields?.pet === true,
    charged: fields?.charged === true,
    expiry: true,
    bound: true,
    text: fields?.text === true,
    guild: fields?.guild === true,
  };
}

/** The profile of an item in the catalogue metadata. */
export function fieldsFor(meta: CatalogMeta | null, itemId: number): ItemFieldsDto | null {
  if (!meta) return null;
  const hit = meta.items.find(([id]) => id === itemId);
  if (!hit) return null;
  return meta.profiles[hit[1]] ?? null;
}

/** An index of `fieldsFor` for a long run of lookups (a kit, the catalogue). */
export function profileIndex(meta: CatalogMeta): ReadonlyMap<number, ItemFieldsDto> {
  const out = new Map<number, ItemFieldsDto>();
  for (const [id, at] of meta.items) {
    const p = meta.profiles[at];
    if (p) out.set(id, p);
  }
  return out;
}

/** The upgrade ceiling of an item, from its profile and the server's limits. */
export function upgradeMax(fields: ItemFieldsDto | null, limits: ItemLimits): number {
  const m = fields?.upgrade_max ?? 0;
  return Math.min(
    m,
    Math.max(limits.upgrade_gear, limits.upgrade_accessory, limits.upgrade_ultimate),
  );
}

/** The number of card sockets an item can have. */
export function piercingMax(fields: ItemFieldsDto | null): number {
  return fields?.piercing_max ?? 0;
}

/** The egg defaults of a pet (`PET_LEVELS[0]` = Egg). */
export function eggPet(limits: ItemLimits): PetRecord {
  return {
    kind: 0,
    level: 0,
    exp: 0,
    energy: limits.pet_energy[1] ?? 0,
    life: 0,
    avail_levels: [0, 0, 0, 0, 0, 0],
    name: '',
  };
}

/**
 * The spec cleaned to the fields the item has: a field that does not apply goes back to its default, so a form that switches
 * from a sword to a potion never sends a stale upgrade level. The server validates the rest (ranges, combinations).
 */
export function cleanSpec(spec: ItemSpec, fields: ItemFieldsDto | null): ItemSpec {
  const v = visibilityOf(fields);
  const out: ItemSpec = { ...spec };
  if (!v.count) out.count = 1;
  if (!v.upgrade) out.upgrade = 0;
  if (!v.element) {
    out.element = null;
    out.element_level = 0;
  }
  if (!v.options) {
    out.random_options = [];
    out.safe = false;
  }
  if (!v.piercing) out.piercing = [];
  if (!v.ultimate) out.ultimate_gems = [];
  if (!v.vis) out.vis = [];
  if (!v.pet) out.pet = null;
  if (!v.charged) out.charged = false;
  if (!v.text) out.text = null;
  if (!v.guild) out.guild_id = null;
  return out;
}

/** Structural equality of two specs (the basket merges equal lines). */
export function sameSpec(a: ItemSpec, b: ItemSpec): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** A deep copy (specs are plain JSON). */
export function cloneSpec(spec: ItemSpec): ItemSpec {
  return JSON.parse(JSON.stringify(spec)) as ItemSpec;
}

/** Is `spec` the plain item (nothing but the id and the count)? */
export function isPlain(spec: ItemSpec): boolean {
  return sameSpec({ ...spec, count: 1 }, blankSpec(spec.item_id));
}

/** The element's display name, `''` for none. */
export function elementName(element: Element | null | undefined): string {
  if (!element) return '';
  const at = ELEMENT_KEYS.indexOf(element);
  return ELEMENTS[at]?.name ?? '';
}

/** The `ItemSpec.element` of an `SAI79` id (`1` = fire ... `5` = earth), `null` for 0 or an unknown id. */
export function elementOfId(id: number): Element | null {
  return ELEMENT_KEYS[id - 1] ?? null;
}

/** The `SAI79` id of an element key (0 = none). */
export function elementId(element: Element | null | undefined): number {
  if (!element) return 0;
  return ELEMENT_KEYS.indexOf(element) + 1;
}

/** `keep_until` as short text: `item`, `none`, `7 d` or an ISO date. */
export function keepText(k: KeepUntil): string {
  if (k === 'item') return '';
  if (k === 'none') return 'permanent';
  if ('duration' in k) return durationText(k.duration.secs);
  return new Date(k.at.unix * 1000).toISOString().slice(0, 10);
}

/** A duration in seconds as the largest whole unit (`90 s`, `5 min`, `3 h`, `7 d`). */
export function durationText(secs: number): string {
  if (secs % 86_400 === 0) return `${String(secs / 86_400)} d`;
  if (secs % 3_600 === 0) return `${String(secs / 3_600)} h`;
  if (secs % 60 === 0) return `${String(secs / 60)} min`;
  return `${String(secs)} s`;
}

/** The extras of a spec in a short list (`+7`, `Fire 5`, `2 options`, ...): the second line of a basket row. */
export function extrasOf(spec: ItemSpec): string[] {
  const out: string[] = [];
  if (spec.upgrade > 0) out.push(`+${String(spec.upgrade)}`);
  if (spec.element) out.push(`${elementName(spec.element)} ${String(spec.element_level)}`);
  if (spec.random_options.length > 0) {
    out.push(
      `${String(spec.random_options.length)} option${spec.random_options.length === 1 ? '' : 's'}${spec.safe ? ' (safe)' : ''}`,
    );
  }
  const cards = spec.piercing.length;
  if (cards > 0) out.push(`${String(cards)} socket${cards === 1 ? '' : 's'}`);
  const gems = spec.ultimate_gems.length;
  if (gems > 0) out.push(`${String(gems)} gem${gems === 1 ? '' : 's'}`);
  if (spec.vis.length > 0) out.push(`${String(spec.vis.length)} vis`);
  if (spec.pet) {
    out.push(`${PET_LEVELS[spec.pet.level] ?? '?'}${spec.pet.name ? ` "${spec.pet.name}"` : ''}`);
  }
  if (spec.charged) out.push('charged');
  const keep = keepText(spec.keep_until);
  if (keep) out.push(keep);
  if (spec.bound === true) out.push('bound');
  if (spec.bound === false) out.push('unbound');
  if (spec.text) out.push('text');
  return out;
}

/** One vis slot with the default lifetime of the item. */
export function blankVis(itemId: number): VisSpec {
  return { item_id: itemId, keep_until: 'item' };
}

/** The recipients of a grant as a short phrase for the confirmation (`3 characters`, `all online players`). */
export function recipientsText(r: Recipients): string {
  if ('characters' in r)
    return `${String(r.characters.length)} character${r.characters.length === 1 ? '' : 's'}`;
  if ('all_online' in r)
    return r.all_online.include_staff ? 'all online players and staff' : 'all online players';
  if ('world' in r) return `everybody in world ${String(r.world)}`;
  if ('party' in r) return `party ${String(r.party)}`;
  return `guild ${String(r.guild)}`;
}
