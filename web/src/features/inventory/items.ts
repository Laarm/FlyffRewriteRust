// The client's item record and the per-field update of `UpdateItem` (Plan/19 §2.1, §4.1; `CMover::UpdateItem`,
// `src:_Common/MoverParam.cpp:3903-4090`; `CItemElem`, WP-CFEAT-10). `InvItem` is the plain-TS form of the wire `ItemElem`
// (64-bit serial and keep time as numbers, the random-option word as a bigint) plus the one client-only field `extra` (the
// part of the stack that a trade, vendor or upgrade window reserved: `m_nExtra`).

import { n64 } from '../../net/int64';
import type { ItemElem, PetData } from '../../gen/proto/flyff/v1/item_types_pb';
import type { StackItem } from './container';

/** `CItemElem::m_byFlag` bits (`Item.h`). */
export const ITEM_FLAG = { EXPIRED: 0x01, BINDS: 0x02, ISUSING: 0x04 } as const;

/** The `UI_*` parameters of `UpdateItem` (`_Common/Mover.h:68-87`). */
export const UI = {
  NUM: 0,
  HP: 1,
  RN: 2,
  AO: 3,
  RAO: 4,
  IR: 5,
  PIERCING_SIZE: 6,
  PIERCING: 7,
  COOLTIME: 8,
  FLAG: 9,
  RANDOMOPTITEMID: 10,
  KEEPTIME: 11,
  REPAIR_NUM: 12,
  ULTIMATE_PIERCING_SIZE: 13,
  ULTIMATE_PIERCING: 14,
  PETVIS_SIZE: 15,
  PETVIS_ITEM: 16,
  PETVIS_ITEMSWAP: 17,
  TRANSFORM_VISPET: 18,
} as const;

/** `MAX_PIERCING_WEAPON`, `MAX_PIERCING_ULTIMATE`, `MAX_VIS` (`Piercing.h`). */
export const MAX_PIERCING_ULTIMATE = 5;
export const MAX_VIS = 9;

export interface InvItem extends StackItem {
  /** `nums` of the wire, the stack count. */
  readonly num: number;
  /** The widened unique serial (js-safe). */
  readonly serial: number;
  readonly itemText: string;
  readonly repairNumber: number;
  /** `m_nHitPoint`: durability (collector battery, gold on the ground). */
  readonly hitPoint: number;
  readonly repair: number;
  /** `GetAbilityOption()`: the upgrade level (+N). */
  readonly abilityOption: number;
  readonly idGuild: number;
  /** `m_bItemResist`: the element. */
  readonly itemResist: number;
  readonly resistAbilityOption: number;
  readonly resistSmItemId: number;
  /** The normal sockets (card item ids, `0` = empty). */
  readonly cards: readonly number[];
  /** The ultimate sockets (gem item ids). */
  readonly gems: readonly number[];
  /** The vis keep times (unix seconds, vis pets only). */
  readonly visExpireAt: readonly number[];
  /** The packed 64-bit awakening / random-option word. */
  readonly randomOpt: bigint;
  /** Unix seconds, `0` = permanent. */
  readonly keepTimeAt: number;
  readonly pet: PetData | undefined;
  readonly transformVisPet: boolean;
  /** `GetExtra()`: client-side reservation (a trade, vendor, upgrade or quick-sell window). */
  readonly extra: number;
}

/** The plain item of a wire `ItemElem`. */
export function itemFromWire(w: ItemElem): InvItem {
  return {
    objId: w.objId,
    itemId: w.itemId,
    serial: n64(w.serial),
    itemText: w.itemText,
    num: w.num,
    repairNumber: w.repairNumber,
    hitPoint: w.hitPoint,
    repair: w.repair,
    flag: w.flag,
    abilityOption: w.abilityOption,
    idGuild: w.idGuild,
    itemResist: w.itemResist,
    resistAbilityOption: w.resistAbilityOption,
    resistSmItemId: w.resistSmItemId,
    cards: w.piercing ? [...w.piercing.cardItemIds] : [],
    gems: w.piercing ? [...w.piercing.ultimateGemIds] : [],
    visExpireAt: w.piercing ? w.piercing.visExpireAt.map((v) => n64(v)) : [],
    charged: w.charged,
    randomOpt: w.randomOptItemId,
    keepTimeAt: n64(w.keepTimeAt),
    pet: w.pet,
    transformVisPet: w.transformVisPet,
    extra: 0,
  };
}

/** `GetAttrOption()`: `ability_option | element << 16 | element_level << 24` (the `EquipInfo.option`). */
export function attrOption(
  it: Pick<InvItem, 'abilityOption' | 'itemResist' | 'resistAbilityOption'>,
): number {
  return it.abilityOption | (it.itemResist << 16) | (it.resistAbilityOption << 24) | 0;
}

/** `LOWORD` / `HIWORD` of a 32-bit value. */
export const loWord = (v: number): number => v & 0xffff;
export const hiWord = (v: number): number => (v >>> 16) & 0xffff;

/** What `applyUpdateItem` needs to know that is not in the item. */
export interface UpdateEnv {
  /** The highest `UI_AO` value of this item: 20, accessories 20, collectors 5 (`dwMax` of `UpdateItem`). */
  aoMax(item: InvItem): number;
  /** The current unix time in seconds (`time_null()`, the keep time and the vis expiry). */
  nowSec(): number;
}

function resized(list: readonly number[], size: number, max: number): number[] {
  const n = Math.max(0, Math.min(size, max));
  const out = list.slice(0, n);
  while (out.length < n) out.push(0);
  return out;
}

function withSlot(list: readonly number[], nth: number, value: number): readonly number[] {
  if (nth < 0 || nth >= list.length) return list;
  const out = [...list];
  out[nth] = value;
  return out;
}

/**
 * The item after `UpdateItem{param, value, time}` for every parameter except the count (`UI_NUM` / `UI_COOLTIME` remove or
 * resize at the container level). `null` = the update changes nothing (an ignored value, an unknown parameter).
 */
export function updateItemField(
  it: InvItem,
  param: number,
  value: number,
  time: number,
  env: UpdateEnv,
): InvItem | null {
  switch (param) {
    case UI.HP:
      return { ...it, hitPoint: value | 0 };
    case UI.REPAIR_NUM:
      return { ...it, repair: value | 0 };
    case UI.RN: {
      if (it.repairNumber >= 100) return null;
      return { ...it, repairNumber: Math.min(value, 100) };
    }
    case UI.AO: {
      const max = env.aoMax(it);
      if (it.abilityOption > max) return null;
      return { ...it, abilityOption: Math.min(value, max) };
    }
    case UI.RAO: {
      if (it.resistAbilityOption > 20) return null;
      return { ...it, resistAbilityOption: Math.min(value, 20) };
    }
    case UI.IR:
      return { ...it, itemResist: value & 0xff };
    case UI.PIERCING_SIZE:
      return { ...it, cards: resized(it.cards, value, 10) };
    case UI.PIERCING:
      return { ...it, cards: withSlot(it.cards, loWord(value), hiWord(value)) };
    case UI.ULTIMATE_PIERCING_SIZE:
      return { ...it, gems: resized(it.gems, value, MAX_PIERCING_ULTIMATE) };
    case UI.ULTIMATE_PIERCING:
      return { ...it, gems: withSlot(it.gems, loWord(value), hiWord(value)) };
    case UI.PETVIS_SIZE:
      return {
        ...it,
        cards: resized(it.cards, value, 10),
        visExpireAt: resized(it.visExpireAt, value, MAX_VIS),
      };
    case UI.PETVIS_ITEM: {
      const nth = loWord(value);
      return {
        ...it,
        cards: withSlot(it.cards, nth, hiWord(value)),
        visExpireAt: withSlot(it.visExpireAt, nth, time * 60 + env.nowSec()),
      };
    }
    case UI.PETVIS_ITEMSWAP: {
      const a = loWord(value);
      const b = hiWord(value);
      if (a >= it.cards.length || b >= it.cards.length) return null;
      const cards = [...it.cards];
      const exp = [...it.visExpireAt];
      [cards[a], cards[b]] = [cards[b] ?? 0, cards[a] ?? 0];
      if (a < exp.length && b < exp.length) [exp[a], exp[b]] = [exp[b] ?? 0, exp[a] ?? 0];
      return { ...it, cards, visExpireAt: exp };
    }
    case UI.TRANSFORM_VISPET:
      return { ...it, transformVisPet: value !== 0, charged: true };
    case UI.RANDOMOPTITEMID:
      // the 32-bit form carries the stat-scroll id (the low byte of the word); the whole word comes with `UpdateItemEx`
      return { ...it, randomOpt: (it.randomOpt & ~0xffn) | BigInt(value & 0xff) };
    case UI.KEEPTIME:
      return { ...it, keepTimeAt: env.nowSec() + value * 60 };
    case UI.FLAG:
      return { ...it, flag: hiWord(value) & 0xff };
    default:
      return null;
  }
}
