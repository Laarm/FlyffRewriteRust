// The typed handle of the wasm `RulesContext` (Plan/16 section 18.4, WP-MOVER-13) and its construction: the
// one place besides `rules/` that touches `RulesContext` of `wasm/pkg`. The handle is structural (snake_case, as
// wasm-bindgen exports it) so that tests can fake it and so that this file compiles against a `pkg` built before
// `RulesContext` existed; the camelCase facade with the budget categories is `rules/facade.ts`.

import { wasmCall } from './call';
import { wasmMemory } from './loader';
import * as pkg from './pkg/flyff_wasm';

/** The four JSON bundles `RulesContext` is built from, as the bytes of the files (the envelopes). */
export interface RulesBundlesBytes {
  readonly tables: Uint8Array;
  readonly items: Uint8Array;
  readonly itemExt: Uint8Array;
  readonly skills: Uint8Array;
}

/** The wasm `RulesContext` as the facade calls it (structural, so tests can fake it). */
export interface RulesContextHandle {
  drain_sync(
    snapshots: Int32Array,
    ops: Int32Array,
    forgets: Uint32Array,
    paramRows: Int32Array,
  ): Float32Array;
  dst_snapshot(objs: Uint32Array, triples: Int32Array): void;
  dst_apply_ops(ops: Int32Array): Uint32Array;
  dst_forget(objs: Uint32Array): void;
  dst_reset_all(): void;
  dst_view(obj: number): Int32Array | undefined;
  mover_params(objs: Uint32Array, inputs: Int32Array): Int32Array;
  derived_stats(obj: number, input: unknown): unknown;
  stat_preview(obj: number, input: unknown, add: Int32Array): unknown;
  move_speed(obj: number, base: number, speedFactor: number): number;
  attack_speed(obj: number, job: number, level: number, dex: number, weaponItem: number): number;
  casting_ani_speed(obj: number): number;
  exp_percent_x100(level: number, exp: number): number;
  equip_check(
    itemId: number,
    randomOptHi: number,
    sex: number,
    job: number,
    level: number,
    rweaponItem: number,
  ): number;
  item_multiplier(itemId: number, abilityOption: number, expired: boolean): number;
  armor_set_level(equipOptions: Int32Array): number;
  /** M4 (WP-MOVER-24): throws `unsupported` until the skill-point rules land. */
  skill_point_cost(skillId: number): number;
  /** M4 (WP-MOVER-24): throws `unsupported` until then; later `>= 0` the cost, `< 0` the negated error code plus one. */
  skill_plan_check(
    cur: Uint32Array,
    plan: Uint32Array,
    level: number,
    job: number,
    sp: number,
  ): number;
  /** M5 (WP-MOVER-18/19): the jobs `UpdateJob` may name for a character of that level and job (Plan/16 section 11.4). */
  fast_job_targets(level: number, job: number): Uint32Array;
  /** M5 (WP-MOVER-19): the client rule that opens the fast job change window. */
  fast_job_window(level: number, job: number, expPercentX100: number): boolean;
  /** M5 (WP-MOVER-23): `0` no, `1` yes, `2` a PK target (the client branch of `IsAttackAble`). */
  client_attackable(selfView: unknown, targetView: unknown, ctx: unknown): number;
  /** M4 (WP-MOVER-22): the `__STATISTICS` lines of the replica of `obj` (`input` undefined = a level-1 vagrant). */
  statistics(obj: number, input: unknown): unknown;
  /** M4 (WP-MOVER-22): the ability line `[min, max]` of an item. */
  item_min_max(itemId: number, abilityOption: number, expired: boolean): Int32Array;
  /** M4 (WP-MOVER-22): every number of one item tooltip in one call. */
  item_tooltip_numbers(input: unknown): unknown;
  /** M4 (WP-MOVER-22): `[unit, tax]` of one piece in a penya shop. */
  buy_unit_price(input: unknown): Float64Array;
  /** M4 (WP-MOVER-22): `[unit, tax]` of one piece sold to an NPC. */
  sell_unit_price(input: unknown): Float64Array;
  /** M4 (WP-MOVER-22): `[unit, tax]` of one piece of a quick sale. */
  quick_sell_unit_price(input: unknown): Float64Array;
  /** M4 (WP-MOVER-22): the Red Chip price of an item, `-1` for none. */
  chip_cost(itemId: number): number;
  /** M4 (WP-MOVER-22): the fee for taking an item out of a mail. */
  mail_storage_fee(cost: number, elapsedS: number): number;
  /** M4 (WP-MOVER-22): the price of a beauty-shop hair change. */
  hair_cost(input: unknown): number;
  /** M4 (WP-MOVER-22): `0` when the item may be traded or vended, else the `TID_*`. */
  tradeable(input: unknown): number;
  /** M4 (WP-ITEM-12): the odds of one upgrade path at a level, `null` at the cap. */
  upgrade_odds(input: unknown): unknown;
  /** M4 (WP-ITEM-12): the branch the server's `Enchant` / `SmeltSafetyReq` takes for a pair of items. */
  upgrade_route(input: unknown): unknown;
  /** M4 (WP-ITEM-12): the costs, caps and bonus levels of the upgrade windows. */
  upgrade_constants(): unknown;
  /** M4 (WP-ITEM-12): the card item that upgrades an element (1..5), `0` for none. */
  element_card(element: number): number;
  free?(): void;
}

/** The constructor of the generated class, when the build has it. */
type RulesContextCtor = new (
  tables: Uint8Array,
  items: Uint8Array,
  itemExt: Uint8Array,
  skills: Uint8Array,
  cfg: unknown,
) => RulesContextHandle;

function constructorOf(module: object): RulesContextCtor | null {
  const c = (module as { RulesContext?: unknown }).RulesContext;
  return typeof c === 'function' ? (c as RulesContextCtor) : null;
}

/**
 * `new RulesContext(tables, items, itemExt, skills, cfg)` (a one-time call outside the frame budget). `null` while the
 * module is not instantiated in this realm or when the build has no `RulesContext`; a bundle that does not parse
 * throws a `WasmCallError`. `cfg` is the object of optional `RulesConfig` switches (camelCase).
 */
export function constructRulesContext(
  bundles: RulesBundlesBytes,
  cfg?: object,
  module: object = pkg,
): RulesContextHandle | null {
  const Ctor = constructorOf(module);
  if (Ctor === null || (module === pkg && wasmMemory() === null)) return null;
  return wasmCall(
    'RulesContext',
    () => new Ctor(bundles.tables, bundles.items, bundles.itemExt, bundles.skills, cfg),
    'O',
  );
}
