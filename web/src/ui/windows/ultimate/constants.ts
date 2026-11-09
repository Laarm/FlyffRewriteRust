// The numbers and the small rules of the ultimate windows that the service, the feature and the tests share with the shells (WP-UI-32): kept out of the
// shell modules so that nothing outside `lazy.ts` imports a shell (`ui/wnd/lazy.ts`).

import type { UltimateItem } from './ports';

/** `MAX_JEWEL`: five Orichalcum and five moonstone slots. */
export const MAX_JEWEL = 10;
/** The slots of one kind in the mixing window. */
export const PER_KIND = 5;
/** `dwLimitLevel1 >= 60`: the lowest level of a general weapon that can be taken apart. */
export const EXTRACT_MIN_LEVEL = 60;
/** `dwLimitLevel1 >= 60`: the lowest level of a weapon the change window takes. */
export const CHANGE_MIN_LEVEL = 60;
/** A unique weapon must be at least +10 (`GetAbilityOption() >= 10`). */
export const UNIQUE_MIN_AO = 10;
/** `MAX_PIERCING_ULTIMATE`: the five sockets of the jewel setting window. */
export const SOCKETS = 5;
/** At most five rows of jewels in the removal window. */
export const JEWEL_ROWS = 5;

/**
 * The server answers nothing to a request it ignores (the pacing of `UltimateTransWeapon` PL-076, a stale item): the windows of the original wait for good.
 * Here a request without an answer is given up after this long and Start works again.
 */
export const ANSWER_TIMEOUT_MS = 10_000;

/** The weapons the extraction slot takes: a general weapon of level 60 or more, or a unique one. */
export function canExtract(item: UltimateItem): boolean {
  const w = item.weapon;
  if (!w) return false;
  return (w.tier === 'general' && w.level >= EXTRACT_MIN_LEVEL) || w.tier === 'unique';
}
