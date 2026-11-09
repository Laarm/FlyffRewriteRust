// The rules facade (Plan/16 section 18.4, Plan/40 section 2): typed camelCase methods over the wasm
// `RulesContext`, every call counted by `wasmCall` in its budget category (Plan/40 section 7.3):
//   * R (cap 1 per loop iteration): `drainSync`, the per-object fallback (`dstSnapshot`, `dstApplyOps`,
//     `dstForget`, `dstResetAll`, `moverParams`) and `derivedStats`;
//   * U (UI-driven, outside the cap): `statPreview`, `equipCheck`, `itemMultiplier`, `moveSpeed`, `attackSpeed`,
//     `castingAniSpeed`, `expPercentX100`, `armorSetLevel`, `fastJobTargets`, `fastJobWindow`, `clientAttackable`, the item and price calls of WP-MOVER-22 (`statistics`,
//     `itemMinMax`, `itemTooltipNumbers`, `buyUnitPrice`, `sellUnitPrice`, `quickSellUnitPrice`, `chipCost`,
//     `mailStorageFee`, `hairCost`, `tradeable`), the upgrade calls of WP-ITEM-12 (`upgradeOdds`, `upgradeRoute`,
//     `upgradeConstants`, `elementCard`) and the debug `dstView`;
//   * O (one-time): the constructor.
// The facade owns nothing the server did not send: the DST replicas inside the context are a cache fed by the
// spawn snapshots and DST messages (see the contract note in `crates/flyff-wasm/src/rules.rs`).

import { wasmCall, type CallCategory } from '../wasm/call';
import { constructRulesContext, type RulesContextHandle } from '../wasm/rulesContext';
import type {
  AttackContext,
  AttackView,
  DerivedStatsJs,
  HairCostInput,
  ItemTooltipInput,
  ItemTooltipNumbers,
  PriceInput,
  PricePair,
  RulesBundles,
  RulesCfgJs,
  StatInputJs,
  StatisticsJs,
  TradeableInput,
  UpgradeConstants,
  UpgradeKind,
  UpgradeOdds,
  UpgradeRoute,
  UpgradeRouteInput,
} from './types';

/** The typed rules of the browser (`ctx.rules`). */
export interface RulesFacade {
  /** `false` when this wasm build lacks `drain_sync`: the caller then uses the batched exports (Plan/40 section 7.10). */
  readonly hasDrainSync: boolean;
  /** **The** boundary call of a network drain: snapshots, ops, forgets, then the param rows (7 `f32` per row). */
  drainSync(
    snapshots: Int32Array,
    ops: Int32Array,
    forgets: Uint32Array,
    paramRows: Int32Array,
  ): Float32Array;
  /** Fallback path: replaces the replicas of `objs` by their snapshots. */
  dstSnapshot(objs: Uint32Array, triples: Int32Array): void;
  /** Fallback path: applies DST ops, returns the objects whose replica changed (ascending). */
  dstApplyOps(ops: Int32Array): Uint32Array;
  /** Fallback path: drops the replicas of despawned movers. */
  dstForget(objs: Uint32Array): void;
  /** Session re-attach and world change: forgets every replica. */
  dstResetAll(): void;
  /** Debug only, never on the drain path: `adj[95]` then `chg[95]`. */
  dstView(obj: number): Int32Array | undefined;
  /** Fallback path: the sim inputs of many movers (8 `i32` per object; slots 5-6 are `f32` bits). */
  moverParams(objs: Uint32Array, inputs: Int32Array): Int32Array;
  /** Every number of the character window for the replica of `obj`. */
  derivedStats(obj: number, input: StatInputJs): DerivedStatsJs;
  /** `derivedStats` with `add = [str, sta, dex, int]` pending stat points. */
  statPreview(
    obj: number,
    input: StatInputJs,
    add: readonly [number, number, number, number],
  ): DerivedStatsJs;
  /** `GetSpeed` with the replica of `obj`. */
  moveSpeed(obj: number, base: number, speedFactor: number): number;
  /** `GetAttackSpeed()`. */
  attackSpeed(obj: number, job: number, level: number, dex: number, weaponItem: number): number;
  /** `GetCastingAniSpeed()`. */
  castingAniSpeed(obj: number): number;
  /** `(int)(exp * 10000 / next_exp(level))` (0..9999); `exp` below 2^53. */
  expPercentX100(level: number, exp: number): number;
  /** `0`: the item may be worn; else the `TID_*` (`EQUIP_SILENT_REFUSAL` when refused without a text). */
  equipCheck(
    itemId: number,
    randomOptHi: number,
    sex: number,
    job: number,
    level: number,
    rweaponItem: number,
  ): number;
  /** `GetItemMultiplier` of an item at an upgrade level. */
  itemMultiplier(itemId: number, abilityOption: number, expired: boolean): number;
  /** The armour +N set level of the 31 attribute options of `EquipInfo`. */
  armorSetLevel(equipOptions: Int32Array | readonly number[]): number;
  /** M4 (WP-MOVER-24): the skill points one level of the skill costs (1 / 2 / 3 by tier); throws for an unknown skill. */
  skillPointCost(skillId: number): number;
  /**
   * M4 (WP-MOVER-24, VR-086): the skill-plan rule of the server (`validate_skill_plan`) on two tables of the 45 job-skill slots,
   * each as 90 words `[skillId, level]` in the server slot order: `>= 0` the cost of the plan in points, `< 0` the refusal
   * `-(code + 1)` (see `SKILL_PLAN_ERROR`); throws for a table of the wrong length.
   */
  skillPlanCheck(
    cur: Uint32Array,
    plan: Uint32Array,
    level: number,
    job: number,
    sp: number,
  ): number;
  /**
   * M5 (WP-MOVER-18/19): the jobs an `UpdateJob` may name for a character of `level` and `job` (the strict table of
   * Plan/16 section 11.4: the server validates with the same function); empty when no fast job change is possible.
   */
  fastJobTargets(level: number, job: number): Uint32Array;
  /**
   * M5 (WP-MOVER-19): does the client open the fast job change window (level 15 vagrant, level 60 expert, level 120
   * pro or master with `expPercentX100 === 9999`).
   */
  fastJobWindow(level: number, job: number, expPercentX100: number): boolean;
  /**
   * M5 (WP-MOVER-23): may the own character attack the clicked object (the client branch of `IsAttackAble`, Plan/43
   * section 7.6): `ATTACKABLE_NO` (0), `ATTACKABLE_YES` (1) or `ATTACKABLE_PK_TARGET` (2, remember it as the last
   * target). A malformed view throws.
   */
  clientAttackable(self: AttackView, target: AttackView, ctx?: AttackContext): number;
  /** The `__STATISTICS` lines of the character window for the replica of `obj` (Plan/16 section 5.12). */
  statistics(obj: number, input?: StatInputJs): StatisticsJs;
  /** The ability line `[min, max]` of an item at an upgrade level (`[0, 0]` for an unknown item). */
  itemMinMax(itemId: number, abilityOption: number, expired: boolean): readonly [number, number];
  /** Every number of one item tooltip in one boundary call (Plan/42 section 9.6). */
  itemTooltipNumbers(input: ItemTooltipInput): ItemTooltipNumbers;
  /** The price of one piece in a penya shop: `GetCost` x shop rate x event factor (Perin fixed, at least 1) and tax. */
  buyUnitPrice(input: PriceInput): PricePair;
  /** What one piece pays when sold to an NPC (a quarter of the cost, at least 1, minus the sales tax). */
  sellUnitPrice(input: PriceInput): PricePair;
  /** What one piece of a quick sale pays (a bound item 1 penya; the tax is still credited). */
  quickSellUnitPrice(input: PriceInput): PricePair;
  /** The Red Chip price of an item, `-1` for none. */
  chipCost(itemId: number): number;
  /** The fee for taking an item out of a mail (`cost` = the item's `GetCost()`, `elapsedS` the age of the mail). */
  mailStorageFee(cost: number, elapsedS: number): number;
  /** The price of a beauty-shop hair change against the current look (the mesh and the colour are charged apart). */
  hairCost(input: HairCostInput): number;
  /** `0`: the item may be offered in a trade or a stall; else the `TID_*` of the refusal. */
  tradeable(input: TradeableInput): number;
  /**
   * WP-ITEM-12: the odds of one upgrade path at the item's current `level` (the shipped tables of the server, Plan/19
   * section 12); `null` at the cap of the path.
   */
  upgradeOdds(kind: UpgradeKind, level: number): UpgradeOdds | null;
  /** WP-ITEM-12: the branch the server's `Enchant` / `SmeltSafetyReq` takes for a pair of items and the items it asks for. */
  upgradeRoute(input: UpgradeRouteInput): UpgradeRoute;
  /** WP-ITEM-12: the costs, caps and bonus levels of the upgrade windows. */
  upgradeConstants(): UpgradeConstants;
  /** WP-ITEM-12: the one card that upgrades an element (1 fire ... 5 earth), `0` for none. */
  elementCard(element: number): number;
  /** Releases the wasm object. */
  free(): void;
}

function call<T>(name: string, category: CallCategory, fn: () => T): T {
  return wasmCall(name, fn, category);
}

/** The facade over a context handle (a real `RulesContext` or a fake). */
export function createRulesFacade(ctx: RulesContextHandle): RulesFacade {
  return {
    hasDrainSync: typeof (ctx as { drain_sync?: unknown }).drain_sync === 'function',
    drainSync: (snapshots, ops, forgets, paramRows) =>
      call('drain_sync', 'R', () => ctx.drain_sync(snapshots, ops, forgets, paramRows)),
    dstSnapshot: (objs, triples) => {
      call('dst_snapshot', 'R', () => {
        ctx.dst_snapshot(objs, triples);
      });
    },
    dstApplyOps: (ops) => call('dst_apply_ops', 'R', () => ctx.dst_apply_ops(ops)),
    dstForget: (objs) => {
      call('dst_forget', 'R', () => {
        ctx.dst_forget(objs);
      });
    },
    dstResetAll: () => {
      call('dst_reset_all', 'R', () => {
        ctx.dst_reset_all();
      });
    },
    dstView: (obj) => call('dst_view', 'U', () => ctx.dst_view(obj)),
    moverParams: (objs, inputs) => call('mover_params', 'R', () => ctx.mover_params(objs, inputs)),
    derivedStats: (obj, input) =>
      call('derived_stats', 'R', () => ctx.derived_stats(obj, input) as DerivedStatsJs),
    statPreview: (obj, input, add) =>
      call(
        'stat_preview',
        'U',
        () => ctx.stat_preview(obj, input, Int32Array.from(add)) as DerivedStatsJs,
      ),
    moveSpeed: (obj, base, speedFactor) =>
      call('move_speed', 'U', () => ctx.move_speed(obj, base, speedFactor)),
    attackSpeed: (obj, job, level, dex, weaponItem) =>
      call('attack_speed', 'U', () => ctx.attack_speed(obj, job, level, dex, weaponItem)),
    castingAniSpeed: (obj) => call('casting_ani_speed', 'U', () => ctx.casting_ani_speed(obj)),
    expPercentX100: (level, exp) =>
      call('exp_percent_x100', 'U', () => ctx.exp_percent_x100(level, exp)),
    equipCheck: (itemId, randomOptHi, sex, job, level, rweaponItem) =>
      call('equip_check', 'U', () =>
        ctx.equip_check(itemId, randomOptHi, sex, job, level, rweaponItem),
      ),
    itemMultiplier: (itemId, abilityOption, expired) =>
      call('item_multiplier', 'U', () => ctx.item_multiplier(itemId, abilityOption, expired)),
    armorSetLevel: (equipOptions) =>
      call('armor_set_level', 'U', () =>
        ctx.armor_set_level(
          equipOptions instanceof Int32Array ? equipOptions : Int32Array.from(equipOptions),
        ),
      ),
    skillPointCost: (skillId) => call('skill_point_cost', 'U', () => ctx.skill_point_cost(skillId)),
    skillPlanCheck: (cur, plan, level, job, sp) =>
      call('skill_plan_check', 'U', () => ctx.skill_plan_check(cur, plan, level, job, sp)),
    fastJobTargets: (level, job) =>
      call('fast_job_targets', 'U', () => ctx.fast_job_targets(level, job)),
    fastJobWindow: (level, job, expPercentX100) =>
      call('fast_job_window', 'U', () => ctx.fast_job_window(level, job, expPercentX100)),
    clientAttackable: (self, target, world) =>
      call('client_attackable', 'U', () => ctx.client_attackable(self, target, world)),
    statistics: (obj, input) =>
      call('statistics', 'U', () => ctx.statistics(obj, input) as StatisticsJs),
    itemMinMax: (itemId, abilityOption, expired) => {
      const r = call('item_min_max', 'U', () => ctx.item_min_max(itemId, abilityOption, expired));
      return [r[0] ?? 0, r[1] ?? 0];
    },
    itemTooltipNumbers: (input) =>
      call(
        'item_tooltip_numbers',
        'U',
        () => ctx.item_tooltip_numbers(input) as ItemTooltipNumbers,
      ),
    buyUnitPrice: (input) =>
      call('buy_unit_price', 'U', () => pricePair(ctx.buy_unit_price(input))),
    sellUnitPrice: (input) =>
      call('sell_unit_price', 'U', () => pricePair(ctx.sell_unit_price(input))),
    quickSellUnitPrice: (input) =>
      call('quick_sell_unit_price', 'U', () => pricePair(ctx.quick_sell_unit_price(input))),
    chipCost: (itemId) => call('chip_cost', 'U', () => ctx.chip_cost(itemId)),
    mailStorageFee: (cost, elapsedS) =>
      call('mail_storage_fee', 'U', () => ctx.mail_storage_fee(cost, elapsedS)),
    hairCost: (input) => call('hair_cost', 'U', () => ctx.hair_cost(input)),
    tradeable: (input) => call('tradeable', 'U', () => ctx.tradeable(input)),
    upgradeOdds: (kind, level) =>
      call('upgrade_odds', 'U', () => ctx.upgrade_odds({ kind, level }) as UpgradeOdds | null),
    upgradeRoute: (input) =>
      call('upgrade_route', 'U', () => ctx.upgrade_route(input) as UpgradeRoute),
    upgradeConstants: () =>
      call('upgrade_constants', 'U', () => ctx.upgrade_constants() as UpgradeConstants),
    elementCard: (element) => call('element_card', 'U', () => ctx.element_card(element)),
    free: () => {
      ctx.free?.();
    },
  };
}

function pricePair(v: ArrayLike<number>): PricePair {
  return { unit: v[0] ?? 0, tax: v[1] ?? 0 };
}

/**
 * Builds the wasm `RulesContext` from the four bundle files (Plan/40 section 3.5: first idle moment after login,
 * before `EnterWorld`; about 300 ms) and wraps it. `null` while the module is not instantiated in this realm or
 * when the build has no `RulesContext` (the caller then keeps `ctx.rules = null`); a bundle that does not parse
 * throws a `WasmCallError`.
 */
export function createRulesContext(
  bundles: RulesBundles,
  cfg?: RulesCfgJs,
  module?: object,
): RulesFacade | null {
  const handle = constructRulesContext(bundles, cfg, module);
  return handle === null ? null : createRulesFacade(handle);
}
