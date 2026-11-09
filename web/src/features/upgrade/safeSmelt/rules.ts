// The item rules of the safe-upgrade windows (Plan/45 §18.2; `src:_Interface/WndNewUpgrade.cpp:300-560` `SetItem` / `CheckScroll`, `WndField.cpp:25000-25830`
// `CWndSmeltSafety::SetItem` / `IsAcceptable*`; WP-CFEAT-24): what may be a target, which material makes which upgrade setting, which protection and smelting
// scroll belong to a setting, the default maximum and the live level of a target. All functions are pure over the item rows and the facts of one item.

import {
  II_GEN_MAT_ELE_DESERT,
  II_GEN_MAT_ELE_CYCLON,
  II_GEN_MAT_ELE_FLAME,
  II_GEN_MAT_ELE_GENERATOR,
  II_GEN_MAT_ELE_RIVER,
  II_GEN_MAT_MOONSTONE,
  II_GEN_MAT_MOONSTONE_1,
  II_GEN_MAT_ORICHALCUM01,
  II_GEN_MAT_ORICHALCUM01_1,
  II_GEN_MAT_ORICHALCUM02,
  II_GEN_TOO_COL_EVENT,
  II_GEN_TOO_COL_NORMAL,
  II_SYS_SYS_SCR_PIEPROT,
  II_SYS_SYS_SCR_SMELPROT,
  II_SYS_SYS_SCR_SMELPROT3,
  II_SYS_SYS_SCR_SMELPROT4,
  II_SYS_SYS_SCR_SMELTING,
  II_SYS_SYS_SCR_SMELTING2,
} from '../../../gen/defs/item';
import {
  IK1_WEAPON,
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_JEWELRY,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK3_ELECARD,
  IK3_ENCHANT,
  IK3_PIERDICE,
  IK3_SHIELD,
  IK3_SMELT_ACCESSORY_MATERIAL,
  IK3_SMELT_GENERAL_MATERIAL,
  IK3_SMELT_ULTIMATE_MATERIAL,
  IK3_SUIT,
  WEAPON_ULTIMATE,
} from '../../../gen/defs/itemkind';
import type { ClientItem } from '../../../data/clientData';
import type { SmeltSetting } from '../../../ui/windows/upgrade/ports';
import { MAX_PIERCING_SUIT, MAX_PIERCING_WEAPON } from '../../inventory/dispatch/upgradeRules';
import type { InvItem } from '../../inventory/items';

/** The columns of an item row the rules read. */
export type SmeltProp = Pick<
  ClientItem,
  'id' | 'item_kind1' | 'item_kind2' | 'item_kind3' | 'refer_stat1'
>;

/** The `SAI79` element numbers. */
export const ELEMENT = { NONE: 0, FIRE: 1, WATER: 2, ELECTRICITY: 3, WIND: 4, EARTH: 5 } as const;

/** `CItemElem::IsCollector( bRefinable )`: the collector tool (and, unless `refinable`, the event collector). */
export function isCollectorId(itemId: number, refinable = false): boolean {
  return itemId === II_GEN_TOO_COL_NORMAL || (!refinable && itemId === II_GEN_TOO_COL_EVENT);
}

/** `IsElementalCard`: the five element cards of the element upgrade. */
export function isElementalCard(itemId: number): boolean {
  return (
    itemId === II_GEN_MAT_ELE_FLAME ||
    itemId === II_GEN_MAT_ELE_RIVER ||
    itemId === II_GEN_MAT_ELE_GENERATOR ||
    itemId === II_GEN_MAT_ELE_DESERT ||
    itemId === II_GEN_MAT_ELE_CYCLON
  );
}

/** The element card that belongs to an element (`WND_ELEMENT`: fire -> flame ... earth -> desert), `0` for none. */
export function cardOfElement(element: number): number {
  switch (element) {
    case ELEMENT.FIRE:
      return II_GEN_MAT_ELE_FLAME;
    case ELEMENT.WATER:
      return II_GEN_MAT_ELE_RIVER;
    case ELEMENT.ELECTRICITY:
      return II_GEN_MAT_ELE_GENERATOR;
    case ELEMENT.WIND:
      return II_GEN_MAT_ELE_CYCLON;
    case ELEMENT.EARTH:
      return II_GEN_MAT_ELE_DESERT;
    default:
      return 0;
  }
}

const isOrichalcum = (id: number): boolean =>
  id === II_GEN_MAT_ORICHALCUM01 || id === II_GEN_MAT_ORICHALCUM01_1;
const isMoonstone = (id: number): boolean =>
  id === II_GEN_MAT_MOONSTONE || id === II_GEN_MAT_MOONSTONE_1;

// ---- the auto window (`CWndNewUpgrade`) --------------------------------------------------------------------------------------------

/** `SetItem`, first branch: a weapon, armour, armour accessory, jewel or collector is a target. */
export function isAutoTarget(prop: SmeltProp, collector: boolean): boolean {
  return (
    prop.item_kind1 === IK1_WEAPON ||
    prop.item_kind2 === IK2_ARMOR ||
    prop.item_kind2 === IK2_ARMORETC ||
    prop.item_kind2 === IK2_JEWELRY ||
    collector
  );
}

/** `SetItem`, second branch: the four smelt materials and the two old resource kinds. */
export function isAutoMaterial(prop: SmeltProp): boolean {
  const k = prop.item_kind3;
  return (
    k === IK3_SMELT_GENERAL_MATERIAL ||
    k === IK3_SMELT_ULTIMATE_MATERIAL ||
    k === IK3_SMELT_ACCESSORY_MATERIAL ||
    k === IK3_ELECARD ||
    k === IK3_ENCHANT ||
    k === IK3_PIERDICE
  );
}

/**
 * The upgrade setting a material makes of a target (the `switch` of the second branch): `null` when the pair does not fit. The `default` branch of the
 * original (the old resource kinds `IK3_ENCHANT` / `IK3_PIERDICE`) decides by item id.
 */
export function materialSetting(
  mat: SmeltProp,
  target: SmeltProp,
  targetCollector: boolean,
): SmeltSetting | null {
  const isWeapon = target.item_kind1 === IK1_WEAPON;
  const generalTarget =
    !targetCollector &&
    target.refer_stat1 !== WEAPON_ULTIMATE &&
    (isWeapon ||
      target.item_kind2 === IK2_ARMOR ||
      target.item_kind2 === IK2_ARMORETC ||
      target.item_kind3 === IK3_SHIELD);
  const ultimateTarget =
    (isWeapon || target.item_kind2 === IK2_ARMOR || target.item_kind2 === IK2_ARMORETC) &&
    target.refer_stat1 === WEAPON_ULTIMATE;
  const jewelTarget = target.item_kind2 === IK2_JEWELRY || targetCollector;
  const socketTarget =
    isWeapon || target.item_kind3 === IK3_SUIT || target.item_kind3 === IK3_SHIELD;
  switch (mat.item_kind3) {
    case IK3_SMELT_GENERAL_MATERIAL:
      return generalTarget ? 'normal' : null;
    case IK3_SMELT_ULTIMATE_MATERIAL:
      return ultimateTarget ? 'ultimate' : null;
    case IK3_SMELT_ACCESSORY_MATERIAL:
      if (jewelTarget) return 'jewel';
      return socketTarget ? 'piercing' : null;
    case IK3_ELECARD:
      return socketTarget ? 'element' : null;
    default:
      break;
  }
  if (isOrichalcum(mat.id)) return generalTarget ? 'normal' : null;
  if (mat.id === II_GEN_MAT_ORICHALCUM02) return ultimateTarget ? 'ultimate' : null;
  if (isMoonstone(mat.id)) {
    if (jewelTarget) return 'jewel';
    return socketTarget ? 'piercing' : null;
  }
  return null;
}

/** `CheckScroll`: the protection scroll that belongs to a setting (`null` for none). */
export function protectionOf(setting: SmeltSetting): number | null {
  switch (setting) {
    case 'normal':
    case 'element':
      return II_SYS_SYS_SCR_SMELPROT;
    case 'jewel':
      return II_SYS_SYS_SCR_SMELPROT4;
    case 'piercing':
      return II_SYS_SYS_SCR_PIEPROT;
    case 'ultimate':
      return II_SYS_SYS_SCR_SMELPROT3;
    case 'none':
      return null;
  }
}

/** `CheckScroll2`: the smelting scroll of a setting (`null` for none). */
export function smeltingScrollOf(setting: SmeltSetting): number | null {
  switch (setting) {
    case 'normal':
      return II_SYS_SYS_SCR_SMELTING;
    case 'element':
      return II_SYS_SYS_SCR_SMELTING2;
    case 'ultimate':
    case 'jewel':
    case 'piercing':
    case 'none':
      return null;
  }
}

/** What a scroll dropped on the auto window becomes with the material and target in place. */
export type ScrollRole = 'protection' | 'scroll' | null;

/**
 * The third branch of `SetItem`: a protection scroll or a smelting scroll for the current material. The conditions are the original's (per material kind first,
 * then the old resource by item id).
 */
export function scrollRole(
  scrollId: number,
  mat: SmeltProp,
  target: SmeltProp,
  targetCollector: boolean,
): ScrollRole {
  const ultimateTarget = target.refer_stat1 === WEAPON_ULTIMATE;
  const k3 = mat.item_kind3;
  let isNormal = k3 === IK3_SMELT_GENERAL_MATERIAL && scrollId === II_SYS_SYS_SCR_SMELPROT;
  let isUlti =
    k3 === IK3_SMELT_ULTIMATE_MATERIAL && scrollId === II_SYS_SYS_SCR_SMELPROT3 && ultimateTarget;
  let isPiercing =
    (target.item_kind1 === IK1_WEAPON || target.item_kind3 === IK3_SUIT) &&
    scrollId === II_SYS_SYS_SCR_PIEPROT &&
    k3 === IK3_SMELT_ACCESSORY_MATERIAL;
  let isAccessory =
    k3 === IK3_SMELT_ACCESSORY_MATERIAL &&
    scrollId === II_SYS_SYS_SCR_SMELPROT4 &&
    (target.item_kind2 === IK2_JEWELRY || targetCollector);
  const isEle =
    k3 === IK3_ELECARD &&
    scrollId === II_SYS_SYS_SCR_SMELPROT &&
    (target.item_kind1 === IK1_WEAPON ||
      target.item_kind3 === IK3_SUIT ||
      target.item_kind3 === IK3_SHIELD);
  let apply = isNormal || isUlti || isPiercing || isAccessory || isEle;
  if (!apply) {
    // old resource
    isNormal = scrollId === II_SYS_SYS_SCR_SMELPROT && isOrichalcum(mat.id);
    isUlti =
      mat.id === II_GEN_MAT_ORICHALCUM02 && scrollId === II_SYS_SYS_SCR_SMELPROT3 && ultimateTarget;
    isPiercing =
      (target.item_kind1 === IK1_WEAPON ||
        target.item_kind3 === IK3_SUIT ||
        target.item_kind3 === IK3_SHIELD) &&
      scrollId === II_SYS_SYS_SCR_PIEPROT &&
      isMoonstone(mat.id);
    isAccessory =
      isMoonstone(mat.id) &&
      scrollId === II_SYS_SYS_SCR_SMELPROT4 &&
      (target.item_kind2 === IK2_JEWELRY || targetCollector);
    apply = isNormal || isUlti || isPiercing || isAccessory || isEle;
  }
  if (apply) return 'protection';
  if (
    (k3 === IK3_SMELT_GENERAL_MATERIAL && scrollId === II_SYS_SYS_SCR_SMELTING) ||
    (scrollId === II_SYS_SYS_SCR_SMELTING2 && k3 === IK3_ELECARD)
  ) {
    return 'scroll';
  }
  return null;
}

/** `GetDefaultMax`: the highest level the setting can reach (the piercing maximum depends on the target). */
export function defaultMax(setting: SmeltSetting, target: SmeltProp | null): number {
  switch (setting) {
    case 'normal':
    case 'ultimate':
      return 10;
    case 'jewel':
    case 'element':
      return 20;
    case 'piercing':
      return target?.item_kind3 === IK3_SUIT ? MAX_PIERCING_SUIT : MAX_PIERCING_WEAPON;
    case 'none':
      return 1;
  }
}

/** `GetCurValue`: the level, element level or sockets of the target, `0` for no setting. */
export function currentValue(setting: SmeltSetting, item: InvItem): number {
  switch (setting) {
    case 'normal':
    case 'ultimate':
    case 'jewel':
      return item.abilityOption;
    case 'element':
      return item.resistAbilityOption;
    case 'piercing':
      return item.cards.length;
    case 'none':
      return 0;
  }
}

// ---- the one-shot window (`CWndSmeltSafety`) --------------------------------------------------------------------------------------

/** The four modes of the vanilla window (`WndMode`). */
export type OneShotMode = 'normal' | 'jewel' | 'piercing' | 'element';

/** `CItemElem::IsDiceRefineryAble`: armour and weapons. */
export function isDiceRefineryAble(prop: SmeltProp): boolean {
  return (
    prop.item_kind2 === IK2_ARMOR ||
    prop.item_kind2 === IK2_WEAPON_MAGIC ||
    prop.item_kind2 === IK2_WEAPON_DIRECT ||
    prop.item_kind2 === IK2_ARMORETC
  );
}

/** `CItemElem::IsEleRefineryAble`: suits and weapons. */
export function isEleRefineryAble(prop: SmeltProp): boolean {
  return (
    prop.item_kind3 === IK3_SUIT ||
    prop.item_kind2 === IK2_WEAPON_MAGIC ||
    prop.item_kind2 === IK2_WEAPON_DIRECT
  );
}

/** The mode's test for the first drop (`bAcceptableItem`): `refuse` is the text id of the refusal. */
export function oneShotTarget(
  mode: OneShotMode,
  prop: SmeltProp,
  collectorRefinable: boolean,
  pierceAble: boolean,
): boolean {
  switch (mode) {
    case 'normal':
      return isDiceRefineryAble(prop);
    case 'jewel':
      return collectorRefinable || prop.item_kind2 === IK2_JEWELRY;
    case 'piercing':
      return pierceAble;
    case 'element':
      return isEleRefineryAble(prop);
  }
}

/** `IsAcceptableMaterial` for the item `mat` (an element stack fixes the card by the target's element, else by the first drop). */
export function oneShotMaterial(
  mode: OneShotMode,
  mat: SmeltProp,
  target: SmeltProp,
  targetElement: number,
  selectedCard: number | null,
  materialsPlaced: boolean,
): { ok: boolean; select: number | null } {
  switch (mode) {
    case 'normal': {
      if (target.refer_stat1 !== WEAPON_ULTIMATE) return { ok: isOrichalcum(mat.id), select: null };
      return { ok: mat.id === II_GEN_MAT_ORICHALCUM02, select: null };
    }
    case 'jewel':
    case 'piercing':
      return { ok: isMoonstone(mat.id), select: null };
    case 'element': {
      const card = cardOfElement(targetElement);
      if (card !== 0) return { ok: mat.id === card, select: null };
      if (!materialsPlaced) {
        // the first card decides the element
        return isElementalCard(mat.id) ? { ok: true, select: mat.id } : { ok: false, select: null };
      }
      return { ok: selectedCard !== null && mat.id === selectedCard, select: null };
    }
  }
}

/** `IsAcceptableScroll1`: the protection scroll of the mode. */
export function oneShotProtection(mode: OneShotMode, scrollId: number, target: SmeltProp): boolean {
  switch (mode) {
    case 'normal':
      return target.refer_stat1 !== WEAPON_ULTIMATE
        ? scrollId === II_SYS_SYS_SCR_SMELPROT
        : scrollId === II_SYS_SYS_SCR_SMELPROT3;
    case 'jewel':
      return scrollId === II_SYS_SYS_SCR_SMELPROT4;
    case 'piercing':
      return scrollId === II_SYS_SYS_SCR_PIEPROT;
    case 'element':
      return scrollId === II_SYS_SYS_SCR_SMELPROT;
  }
}

/** `GENERAL_NON_USING_SCROLL2_LEVEL` / `ELEMENTAL_NON_USING_SCROLL2_LEVEL` (`WndField.h:2528`): from this level on the smelting scroll is no use. */
export const GENERAL_NON_USING_SCROLL2_LEVEL = 7;
export const ELEMENTAL_NON_USING_SCROLL2_LEVEL = 10;

/** `IsAcceptableScroll2`: the smelting scroll (normal and element mode, below the level that makes it useless). */
export function oneShotSmelting(
  mode: OneShotMode,
  scrollId: number,
  target: SmeltProp,
  item: InvItem,
): boolean {
  if (mode === 'normal') {
    return (
      item.abilityOption < GENERAL_NON_USING_SCROLL2_LEVEL &&
      target.refer_stat1 !== WEAPON_ULTIMATE &&
      scrollId === II_SYS_SYS_SCR_SMELTING
    );
  }
  if (mode === 'element') {
    return (
      item.resistAbilityOption < ELEMENTAL_NON_USING_SCROLL2_LEVEL &&
      scrollId === II_SYS_SYS_SCR_SMELTING2
    );
  }
  return false;
}
