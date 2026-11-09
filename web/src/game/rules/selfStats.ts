// The inputs and outputs of the own character's `derived_stats` call (Plan/16 section 18.6, Plan/40 section 7.10; WP-CLIENT-30):
// `StatInputJs` is built from what the client knows (the self store, the equipment of the visual state), the result goes
// back into `selfStore` (the maxima of the pools and the flat `derived` record the character window reads).

import { PARTS_RWEAPON } from '../../gen/defs/neuz';
import type { DerivedStatsJs, EquipJs, StatInputJs } from '../../rules';
import type { DerivedStatsView, SelfState } from '../../stores/self';
import type { MoverVisualState } from '../entities';

/** `CItemElem::m_byFlag` bit 0: the item expired. */
const ITEM_FLAG_EXPIRED = 0x01;

/** The worn items of a visual state as the rules want them (`PARTS_*` order, so the input is deterministic). */
export function equipOf(visual: Pick<MoverVisualState, 'equip'> | undefined): EquipJs[] {
  const out: EquipJs[] = [];
  if (!visual) return out;
  const parts = [...visual.equip.keys()].sort((a, b) => a - b);
  for (const part of parts) {
    const e = visual.equip.get(part);
    if (!e || e.itemId === 0 || e.itemId === 0xffff_ffff) continue;
    out.push({
      part,
      itemId: e.itemId,
      // `EquipInfo.option` is `GetAttrOption()` (`ability_option | element << 16 | element_level << 24`): the rules want the +N
      abilityOption: e.option & 0xffff,
      expired: (e.flag & ITEM_FLAG_EXPIRED) !== 0,
    });
  }
  return out;
}

/** `StatInputJs` of the own character. */
export function statInputOf(
  self: Pick<SelfState, 'level' | 'job' | 'base'>,
  visual: Pick<MoverVisualState, 'equip'> | undefined,
): StatInputJs {
  return {
    isPlayer: true,
    level: self.level,
    job: self.job,
    base: [self.base.str, self.base.sta, self.base.dex, self.base.int],
    equip: equipOf(visual),
    weaponPart: PARTS_RWEAPON,
  };
}

/** The item id in the active weapon slot (`0` = bare hands). */
export function weaponItemOf(visual: Pick<MoverVisualState, 'equip'> | undefined): number {
  const w = visual?.equip.get(PARTS_RWEAPON);
  return w && w.itemId !== 0xffff_ffff ? w.itemId : 0;
}

/** The keys of the inputs a derived record was computed for (`for.level`, `for.job`, `for.str` ... `for.int`). */
const FOR_KEYS = ['for.level', 'for.job', 'for.str', 'for.sta', 'for.dex', 'for.int'] as const;

/**
 * The numbers of `DerivedStatsJs` as the flat record of `selfStore.derived` (`statistics.*` for the nested block). With the
 * `input` the record was computed for, the `for.*` keys say which level, job and base stats the numbers belong to, so a reader
 * can tell a record that is one frame behind a base-stat change ({@link derivedMatches}).
 */
export function flattenDerived(
  d: DerivedStatsJs,
  input?: Pick<StatInputJs, 'level' | 'job' | 'base'>,
): DerivedStatsView {
  const out: Record<string, number> = {};
  for (const [k, v] of Object.entries(d)) {
    if (typeof v === 'number') out[k] = v;
  }
  for (const [k, v] of Object.entries(d.statistics)) {
    if (typeof v === 'number') out[`statistics.${k}`] = v;
  }
  if (input) {
    out['for.level'] = input.level;
    out['for.job'] = input.job;
    [out['for.str'], out['for.sta'], out['for.dex'], out['for.int']] = input.base;
  }
  return out;
}

/**
 * `false` when `derived` was computed for other level, job or base stats than `self` has now: the rules call that follows a
 * base-stat change runs one loop iteration later (R slot, at most every 100 ms), so for that long the record is stale. A
 * record without `for.*` keys (a fake, an older writer) always matches.
 */
export function derivedMatches(
  derived: DerivedStatsView,
  self: Pick<SelfState, 'level' | 'job' | 'base'>,
): boolean {
  const want = [self.level, self.job, self.base.str, self.base.sta, self.base.dex, self.base.int];
  return FOR_KEYS.every((k, i) => derived[k] === undefined || derived[k] === want[i]);
}
