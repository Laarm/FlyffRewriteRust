// Types of the rules facade (Plan/16 section 18.4, WP-MOVER-13): the inputs and outputs of `RulesContext`
// (`crates/flyff-wasm/src/rules.rs`). Shapes are camelCase mirrors of the serde structs of the Rust side; the
// array layouts are the `Int32Array`/`Float32Array` words documented there.

/** One worn item of {@link StatInputJs}. */
export interface EquipJs {
  /** `PARTS_*` (0..30). */
  readonly part: number;
  /** The item id; an id the item bundle does not know is ignored. */
  readonly itemId: number;
  /** `GetAbilityOption()`: the upgrade level. */
  readonly abilityOption: number;
  /** `CItemElem::expired`: contributes nothing. */
  readonly expired: boolean;
}

/**
 * The input of `derivedStats` / `statPreview` (16 section 18.4): the character as the window knows it. The DST part
 * of the numbers comes from the replica of the object passed next to it.
 */
export interface StatInputJs {
  /** `IsPlayer()`; only players are shown in the windows of M3. */
  readonly isPlayer: boolean;
  readonly level: number;
  /** `JOB_*`, `-1` for none. */
  readonly job: number;
  /** `[str, sta, dex, int]` (the base stats, not `GetStr()`). */
  readonly base: readonly [number, number, number, number];
  /** The worn items. */
  readonly equip: readonly EquipJs[];
  /** The part of the active weapon (`PARTS_RWEAPON` 10 for the normal case). */
  readonly weaponPart: number;
}

/** The `__STATISTICS` numbers of the character window. */
export interface StatisticsJs {
  readonly speedRun: number;
  readonly criticalRate: number;
  readonly spellCastSpeed: number;
  readonly hitRate: number;
  readonly blockMelee: number;
  readonly blockRange: number;
  readonly pvmDmg: number;
  readonly pvpDmg: number;
}

/** Every number of the character window (`flyff_rules::stats::DerivedStats`). */
export interface DerivedStatsJs {
  readonly str: number;
  readonly sta: number;
  readonly dex: number;
  readonly int: number;
  readonly maxHp: number;
  readonly maxMp: number;
  readonly maxFp: number;
  readonly hpRecovery: number;
  readonly mpRecovery: number;
  readonly fpRecovery: number;
  readonly attackMin: number;
  readonly attackMax: number;
  readonly defenseShown: number;
  readonly hitRating: number;
  readonly parry: number;
  readonly criticalProb: number;
  readonly attackSpeed: number;
  readonly castingAniSpeed: number;
  readonly moveSpeed: number;
  readonly statistics: StatisticsJs;
}

/** The `RulesConfig` switches (`features.*`, `gameplay.*`); a missing one takes the repack default. */
export interface RulesCfgJs {
  readonly bowdmgPropjob?: boolean;
  readonly wandAutoattacks?: boolean;
  readonly betterArmorBonus?: boolean;
  readonly autoSkillMax?: boolean;
  readonly fixGetHpMpFpPoint?: boolean;
  readonly removeMaxRecoverRate?: boolean;
  readonly healOverTime?: boolean;
  readonly setMaxSpeed?: number;
  readonly setMaxCast?: number;
  readonly setMaxCrit?: number;
  readonly maxBlockValue?: number;
  readonly maxBlockValueGw?: number;
  /** `features.only_armor_awakes` (default true): which items carry a random-option kind in the tooltip numbers. */
  readonly onlyArmorAwakes?: boolean;
}

/** The four JSON bundles `RulesContext` is built from, as the bytes of the files (the envelopes). */
export interface RulesBundles {
  readonly tables: Uint8Array;
  readonly items: Uint8Array;
  readonly itemExt: Uint8Array;
  readonly skills: Uint8Array;
}

/** The kinds of a DST op, as the words `dst_apply_ops` takes. */
export const DST_OP = {
  /** `SetDestParam{dst, adj, chg}`: `a = adj`, `b = chg`. */
  set: 0,
  /** `ResetDestParam{dst, adj}`: `a = adj`, `b = 0`. */
  reset: 1,
  /** `ResetDestParamSync{dst, adj, total}` (SPEED only): `a = adj`, `b = total`. */
  sync: 2,
} as const;

/** One `(dst, adj, chg)` triple of a spawn snapshot (`MoverCommon.dst_params`). */
export interface DstTriple {
  readonly dst: number;
  readonly adj: number;
  readonly chg: number;
}

/** One param row of `drainSync` (the dirty mover's attack-speed inputs). */
export interface ParamRowIn {
  readonly obj: number;
  /** `JOB_*`, `-1` for an NPC. */
  readonly job: number;
  readonly level: number;
  readonly baseDex: number;
  /** The active weapon's item id, `0` for unarmed and NPCs. */
  readonly weaponItem: number;
}

/** Words per `drainSync` result row. */
export const DRAIN_ROW_WORDS = 7;
/** Words per `moverParams` result row. */
export const PARAMS_ROW_WORDS = 8;

/** One decoded `drainSync` result row. */
export interface DrainRow {
  readonly speedAdjPct: number;
  /** `chg(DST_SPEED) != NULL_CHGPARAM`: the mover is rooted. */
  readonly speedChgSet: boolean;
  readonly chrState: number;
  readonly hawkeyeRate: number;
  readonly jumping: number;
  readonly attackSpeed: number;
  readonly castingAniSpeed: number;
}

/** `equipCheck` answers this for a refusal without a text (an arrow without a bow). */
export const EQUIP_SILENT_REFUSAL = 0xffffffff;

// ---- items, prices, tooltips (WP-MOVER-22, Plan/16 section 18.4 M4 rows, Plan/42 section 9.6) ----

/** A `[dst, adj]` pair (`DST_*` and its signed value). */
export type DstPair = readonly [dst: number, adj: number];

/** The viewer of a tooltip: the requirement lines (`IsEquipAble`'s sex, job and level) are evaluated for it. */
export interface TooltipOwnerJs {
  /** 0 male, 1 female. */
  readonly sex: number;
  readonly job: number;
  readonly level: number;
  /** The item in the right hand (an arrow needs a bow); 0 = none. */
  readonly rweaponItem: number;
}

/** One item the viewer wears (`m_aEquipInfo[part]`), for the set count. */
export interface TooltipWornJs {
  /** `PARTS_*`. */
  readonly part: number;
  readonly itemId: number;
  readonly expired: boolean;
}

/** The shop the tooltip is shown for; a missing field is the neutral value (rate and factors 1.0, no tax). */
export interface TooltipShopJs {
  /** `rates.shop_cost` (`prj.m_fShopCost`); `null` = 1.0. */
  readonly shopRate?: number | null;
  /** `EventLua.GetShopBuyFactor()`. */
  readonly buyFactor?: number;
  /** `EventLua.GetShopSellFactor()`. */
  readonly sellFactor?: number;
  /** The purchase tax rate of the continent the viewer stands on (0 = none). */
  readonly purchaseTaxRate?: number;
  /** The sales tax rate (0 = none). */
  readonly salesTaxRate?: number;
}

/**
 * The input of `itemTooltipNumbers` (Plan/42 section 9.6): one item instance and its viewer. Everything but
 * `itemId` is optional. The 64-bit random-option word travels as two 32-bit halves (see
 * {@link splitRandomOptionWord}); it never goes through a JavaScript number.
 */
export interface ItemTooltipInput {
  readonly itemId: number;
  /** `GetAbilityOption()`: the upgrade level. */
  readonly abilityOption?: number;
  readonly expired?: boolean;
  /** The element (`SAI79::ePropType`), 0 = none. */
  readonly element?: number;
  readonly elementLevel?: number;
  readonly randomOptionHi?: number;
  readonly randomOptionLo?: number;
  /** The item is an egg whose pet is level C or better (a system-pet word). */
  readonly eggLevelC?: boolean;
  /** The card item of each socket (0 = empty). */
  readonly piercing?: readonly number[];
  /** The ability item of each ultimate socket (0 = empty). */
  readonly ultimateGems?: readonly number[];
  readonly owner?: TooltipOwnerJs;
  readonly worn?: readonly TooltipWornJs[];
  readonly shop?: TooltipShopJs;
}

/** `awakening` | `blessing` | `systemPet` | `eatPet`. */
export type RandomOptionKind = 'awakening' | 'blessing' | 'systemPet' | 'eatPet';

/** One decoded random-option line. */
export interface RandomOptionLine {
  /** The table the item's word belongs to; `null` for an item without a kind. */
  readonly kind: RandomOptionKind | null;
  readonly dst: number;
  readonly adj: number;
  /** The largest positive value listed for `(kind, dst)` in the random-option tables (0 = unlisted): the line is
   * the table maximum when `maxAdj !== 0 && maxAdj <= adj` (`features.max_awake_color`). */
  readonly maxAdj: number;
}

/** The set the item belongs to (`PutSetItemOpt`). */
export interface SetSummary {
  readonly setId: number;
  readonly name: string;
  /** Pieces worn (right id in its slot, not expired). */
  readonly worn: number;
  readonly total: number;
  readonly members: readonly {
    readonly itemId: number;
    readonly parts: number;
    readonly worn: boolean;
  }[];
  /** The active bonuses: every row with `equiped <= worn`, merged by DST. */
  readonly activeDsts: readonly DstPair[];
}

/** Everything `itemTooltipNumbers` answers (Plan/42 section 9.6). */
export interface ItemTooltipNumbers {
  /** The item exists; `false` zeroes everything else. */
  readonly known: boolean;
  /** `GetItemMultiplier` as an `f32` (compare thresholds with `Math.fround`). */
  readonly multiplier: number;
  /** The ability line `(int)(min * f) + int(opt^1.5)` and the same for max. */
  readonly ability: { readonly min: number; readonly max: number };
  /** 0 = the viewer meets the requirements, else the `TID_*` (`0xffffffff`: refused without a text). */
  readonly equipDenied: number;
  readonly prices: {
    /** `GetCost()` at the upgrade level, `-1` when unpriced. */
    readonly cost: number;
    /** The price of one piece in a penya shop before tax (>= 1; the Perin is 100 000 000). */
    readonly buyUnit: number;
    readonly buyTax: number;
    /** What one piece pays when sold to an NPC (after the sales tax). */
    readonly sellUnit: number;
    readonly sellTax: number;
    /** The Red Chip price, `-1` when absent. */
    readonly chipCost: number;
  };
  /** A protected awakening is pending (the lines are drawn struck out). */
  readonly awakeningProtected: boolean;
  readonly randomOptions: readonly RandomOptionLine[];
  /** The stat-bonus item of the word's low byte (the "of the Tiger" prefix), `null` for none. */
  readonly randomStat: {
    readonly id: number;
    readonly name: string;
    readonly dsts: readonly DstPair[];
  } | null;
  readonly set: SetSummary | null;
  /** The merged parameters of the socketed cards. */
  readonly piercingDsts: readonly DstPair[];
  /** The parameters of the ultimate gems. */
  readonly gemDsts: readonly DstPair[];
  /** `(weak, -2 * level)`, `(strong, +2 * level)` of an element upgrade. */
  readonly elementResistDsts: readonly DstPair[];
}

/**
 * The input of `buyUnitPrice`, `sellUnitPrice` and `quickSellUnitPrice`: an explicit `cost` (`GetCost()`), or an
 * `itemId` with its `abilityOption` (then the item table decides).
 */
export interface PriceInput {
  readonly cost?: number;
  readonly itemId?: number;
  readonly abilityOption?: number;
  /** `rates.shop_cost`; `null` = 1.0 (buy only). */
  readonly shopRate?: number | null;
  /** The event factor; `null` = 1.0. */
  readonly eventFactor?: number | null;
  /** The Perin price rule; `null` = `itemId` is the Perin. */
  readonly perin?: boolean | null;
  /** The tax rate that applies; `null` = not taxable. For a named `itemId` the exempt items (Perin, Red Chip, the
   * awakening scrolls) pay none whatever is passed. */
  readonly taxRate?: number | null;
  /** The item is bound (a quick sale pays 1 penya). */
  readonly binds?: boolean;
}

/** `[unit, tax]` of one piece. */
export interface PricePair {
  readonly unit: number;
  readonly tax: number;
}

/** The input of `hairCost`: the current look and the requested one. */
export interface HairCostInput {
  readonly curMesh: number;
  /** The stored colour, ARGB. */
  readonly curColor: number;
  readonly rgb: readonly [number, number, number];
  readonly hair: number;
}

/** The input of `tradeable` (`0` = may be offered, else the `TID_*` of the refusal). */
export interface TradeableInput {
  readonly itemId: number;
  readonly reserved?: boolean;
  readonly flags?: number;
  readonly keepUntil?: number;
  readonly equipped?: boolean;
  readonly inUse?: boolean;
  readonly guildId?: number;
  readonly bindsByOption?: boolean;
  /** The current unix time in seconds. */
  readonly now: number;
}

/**
 * One mover as `clientAttackable` sees it (Plan/43 section 7.6, WP-MOVER-23); the own character and the clicked object
 * share the flat shape and every field is optional (a missing one is the neutral value: a level-1 mover on the
 * ground, no duel, no war, no guild).
 */
export interface AttackView {
  /** `'player'` (the default), `'npc'` or `'other'` (a ground item or a ctrl); only the target's kind matters. */
  readonly kind?: 'player' | 'npc' | 'other';
  readonly id?: number;
  readonly flying?: boolean;
  /** `pk_propensity > 0`. */
  readonly chaotic?: boolean;
  readonly duelState?: number;
  /** `0xFFFFFFFF` = none. */
  readonly duelOther?: number;
  readonly duelParty?: number;
  readonly partyId?: number;
  readonly guildId?: number;
  readonly warId?: number;
  readonly guildCombatState?: number;
  readonly level?: number;
  /** The `RA_*` class of the region the character stands in (`GetPKPVPRegionAttr`). */
  readonly regionAttr?: number;
  readonly x?: number;
  readonly z?: number;
  /** The own character only: `IK3_TEXT_DISGUISE` buff. */
  readonly textDisguise?: boolean;
  /** The own character only: an item cast runs (`STATE_BASEMOTION_MODE`). */
  readonly baseMotion?: boolean;
  /** The own character only: the attack key is held. */
  readonly forceAttack?: boolean;
  /** The own character only: the alt key is held. */
  readonly altKey?: boolean;
  /** A player target only: a vendor stall is open. */
  readonly vendorOpen?: boolean;
  /** A player target only: it attacked the own character recently. */
  readonly pking?: boolean;
  /** An NPC target only. */
  readonly dead?: boolean;
  readonly killable?: boolean;
  readonly peaceful?: boolean;
  /** `dwClass == RANK_GUARD`. */
  readonly guard?: boolean;
  /** `nChaotic` of the NPC's prop (guards only). */
  readonly alignment?: number;
  /** `dwFlying == 1` (an NPC target only). */
  readonly flyingNpc?: boolean;
}

/** The world state `clientAttackable` reads: the event flags and the kind of world (all optional). */
export interface AttackContext {
  /** `EVE_18` (default true). */
  readonly eve18?: boolean;
  /** `events.pk`. */
  readonly eventsPk?: boolean;
  readonly guildWar?: boolean;
  readonly schoolBattle?: boolean;
  readonly guildCombat?: boolean;
  readonly guildCombat1to1?: boolean;
  readonly worldIsGuildWar?: boolean;
  readonly worldIsArena?: boolean;
  readonly gc1to1Target?: boolean;
  /** `PKSetting.LIMITE_LEVEL` (default 20). */
  readonly pkLimitLevel?: number;
}

/** `clientAttackable`: the target cannot be attacked. */
export const ATTACKABLE_NO = 0;
/** `clientAttackable`: the target can be attacked. */
export const ATTACKABLE_YES = 1;
/** `clientAttackable`: a player the PK rules allow (remember it as the last target). */
export const ATTACKABLE_PK_TARGET = 2;

/**
 * Splits the signed 64-bit random-option word of an item (`ItemElem.random_opt_item_id`) into the two 32-bit halves
 * `itemTooltipNumbers` takes.
 */
export function splitRandomOptionWord(word: bigint): {
  readonly randomOptionHi: number;
  readonly randomOptionLo: number;
} {
  const u = BigInt.asUintN(64, word);
  return {
    randomOptionHi: Number(u >> 32n),
    randomOptionLo: Number(u & 0xffff_ffffn),
  };
}

/** `MAX_SKILL_JOB`: the slots of the job-skill table of a player (vagrant 0..3, expert 3..23, pro 23..43, master 43, hero 44). */
export const MAX_SKILL_JOB = 45;

/**
 * The refusals of `skillPlanCheck` (VR-086, `SkillPlanError::code`): the export answers `-(code + 1)`. `NOTHING_TO_DO` is ignored
 * by the server without an answer, every other code is answered with `TID_RESKILLPOINT_ERROR`.
 */
export const SKILL_PLAN_ERROR = {
  WRONG_SKILL: 0,
  LEVEL_DOWN: 1,
  ABOVE_MAX: 2,
  REQ_LEVEL: 3,
  PREREQUISITE: 4,
  NOT_ENOUGH_POINTS: 5,
  NOTHING_TO_DO: 6,
} as const;

/** The refusal code of a `skillPlanCheck` answer, `null` for a legal plan (`>= 0` = its cost). */
export function skillPlanErrorCode(answer: number): number | null {
  return answer >= 0 ? null : -answer - 1;
}

// ---- upgrades (WP-ITEM-11, WP-ITEM-12; Plan/19 section 12) ------------------------------------------------------

/** The upgrade paths `upgradeOdds` knows: the roll rule differs (`accessory` succeeds when `r < p`, the others fail when `r > p`). */
export type UpgradeKind = 'general' | 'element' | 'accessory' | 'accessorySafe' | 'collector';

/** The odds of one path at a level. */
export interface UpgradeOdds {
  /** The table value `p`. */
  readonly p: number;
  /** The roll space: 10000, or 1000 for the collector. */
  readonly space: number;
  /** The success chance in parts per `space` after the roll rule of the path. */
  readonly chance: number;
  /** The cap of the path (the item cannot go further). */
  readonly maxLevel: number;
}

/** The pair of items `upgradeRoute` dispatches (every flag optional). */
export interface UpgradeRouteInput {
  readonly targetItemId: number;
  readonly materialItemId: number;
  /** `SmeltSafetyReq` instead of the legacy `Enchant`. */
  readonly safe?: boolean;
  /** The target is an accessory of `accessory.inc`. */
  readonly targetIsAccessory?: boolean;
  /** The sockets the target has. */
  readonly targetPiercingSize?: number;
  readonly targetIsVisPet?: boolean;
  /** `features.better_safe_upgrades` (default true). */
  readonly betterSafeUpgrades?: boolean;
  /** `features.no_piercing` (default true). */
  readonly noPiercing?: boolean;
}

/** The branch the server dispatches to. */
export type UpgradeBranch =
  'general' | 'element' | 'accessory' | 'collector' | 'piercingSize' | 'ultimate' | 'none';

/** The route of a pair: the branch and, for the safe path, the items it asks for (`0` = none). */
export interface UpgradeRoute {
  readonly branch: UpgradeBranch;
  /** The protection item the safe path consumes. */
  readonly protection: number;
  /** The smelting scroll the safe path accepts as its optional fourth item. */
  readonly smeltScroll: number;
  /** The cap of the branch. */
  readonly maxLevel: number;
}

/** The costs, caps and bonus levels of the upgrade windows. */
export interface UpgradeConstants {
  readonly removeElementCost: number;
  readonly awakeningCost: number;
  readonly piercingSizeCost: number;
  readonly piercingRemoveCost: number;
  readonly maxGeneral: number;
  readonly maxElement: number;
  readonly maxAccessory: number;
  readonly maxCollector: number;
  readonly destroyFromLevel: number;
  readonly smeltingBonus: number;
  readonly superSmeltingBonus: number;
  readonly generalBonusBelow: number;
  readonly elementBonusBelow: number;
}
