// The FREE item rule exports of the wasm module (Plan/19 §22, §3.4, §3.5, §5.4, §5.12, WP-ITEM-15 / WP-ITEM-20;
// `crates/flyff-wasm/src/item_rules.rs`): pocket sizes and availability, the order of the inventory sort, the plan of a
// banded EXP scroll and the angel goal, shared with the server (`flyff-rules::item::buffitem`). They need neither the
// bundles nor `RulesContext`. Feature-detected like the lobby rules: `createWasmItemRules()` is `null` while the module
// is not instantiated in this realm or when the build lacks the exports.

import * as pkg from './pkg/flyff_wasm';
import { wasmMemory } from './loader';

/** `code` of `ExpScrollPlan`: the scroll works. */
export const EXP_APPLY = 0;
/** The level is outside the band of the scroll. */
export const EXP_WRONG_BAND = 90;
/** All tiers the level allows are taken. */
export const EXP_FULL = 91;
/** A special (`AMPESS`) buff blocks every other scroll. */
export const EXP_BLOCKED = 92;

/** What using a banded EXP scroll comes to (before the exclusivity with the legacy scrolls). */
export interface ExpScrollPlan {
  readonly code: number;
  /** The tier that gets the buff (the offset added to the scroll id); -1 unless `code` is 0. */
  readonly slot: number;
  /** The bonus of the buff in percent (50, 100, 150); 0 unless `code` is 0. */
  readonly percent: number;
  /** The tiers the level allows (0 for a wrong band or a blocked scroll). */
  readonly maxTiers: number;
  /** The band (low, high) for `EXP_WRONG_BAND` (of the scroll) and `EXP_FULL` (of the level); 0 otherwise. */
  readonly band: readonly [number, number];
}

export interface ItemRules {
  /** The slots of pocket `index`: 18/42/42 with `biggerPocket`, else 6/24/24; 0 for no such pocket. */
  pocketCapacity(index: number, biggerPocket: boolean): number;
  pocketAvailable(
    permExtrabags: boolean,
    index: number,
    exists: boolean,
    expired: boolean,
  ): boolean;
  /** `kinds[i]` = `item_kind3` of the i-th usable bag item in slot order; the indices in the order `SortInventory` moves them. */
  inventorySortOrder(kinds: readonly number[]): number[];
  /** `takenMask`: bit i set = tier i holds a buff; `buffCount` = the number of EXP scroll buffs. */
  expScrollPlan(
    scroll: number,
    level: number,
    takenMask: number,
    buffCount: number,
    specialCount: number,
  ): ExpScrollPlan;
  /** The angel EXP `AngelBuff` needs: `exp1 / 100 * percent` (integers). */
  angelGoalExp(exp1: number, percent: number): number;
}

interface Exports {
  pocket_capacity: (index: number, biggerPocket: boolean) => number;
  pocket_available: (perm: boolean, index: number, exists: boolean, expired: boolean) => boolean;
  inventory_sort_order: (kinds: Uint32Array) => Uint32Array;
  exp_scroll_plan: (
    scroll: number,
    level: number,
    takenMask: number,
    buffCount: number,
    specialCount: number,
  ) => Int32Array;
  angel_goal_exp: (exp1: number, percent: number) => number;
}

function exportsOf(module: object): Exports | null {
  const m = module as Partial<Record<keyof Exports, unknown>>;
  for (const k of [
    'pocket_capacity',
    'pocket_available',
    'inventory_sort_order',
    'exp_scroll_plan',
    'angel_goal_exp',
  ] as const) {
    if (typeof m[k] !== 'function') return null;
  }
  return module as Exports;
}

/** `ItemRules` over the instantiated module, or `null` (not instantiated, or the exports are missing). */
export function createWasmItemRules(module: object = pkg): ItemRules | null {
  const e = exportsOf(module);
  if (e === null || (module === pkg && wasmMemory() === null)) return null;
  return {
    pocketCapacity: (index, bigger) => e.pocket_capacity(index, bigger),
    pocketAvailable: (perm, index, exists, expired) =>
      e.pocket_available(perm, index, exists, expired),
    inventorySortOrder: (kinds) => Array.from(e.inventory_sort_order(Uint32Array.from(kinds))),
    expScrollPlan(scroll, level, takenMask, buffCount, specialCount) {
      const r = e.exp_scroll_plan(scroll, level, takenMask, buffCount, specialCount);
      return {
        code: r[0] ?? 0,
        slot: r[1] ?? -1,
        percent: r[2] ?? 0,
        maxTiers: r[3] ?? 0,
        band: [r[4] ?? 0, r[5] ?? 0],
      };
    },
    angelGoalExp: (exp1, percent) => e.angel_goal_exp(exp1, percent),
  };
}
