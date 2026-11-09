// The item rules the upgrade cursor validates with (Plan/45 §9.3; `CItemElem::IsPierceAble`, `IsEleRefineryAble`,
// `IsDiceRefineryAble`, `src:_Common/Item.cpp:359-382, 739-796`). They are rules-crate functions in the plan (`isPierceAble`,
// `isEleRefineryAble`, `isDiceRefineryAble` of the `RulesContext`, 45 §2.7); the wasm module does not export them yet, so this is
// the TS port the dispatch uses until it does (`createUpgradeRules` prefers an export when the facade has one).

import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK3_SHIELD,
  IK3_SOCKETCARD,
  IK3_SOCKETCARD2,
  IK3_SUIT,
  IK3_VIS,
  PET_VIS,
  IK3_PET,
} from '../../../gen/defs/itemkind';
import type { ClientItem } from '../../../data/clientData';
import type { InvItem } from '../items';
import type { UpgradeRules } from './env';

/** `MAX_PIERCING_SUIT`, `MAX_PIERCING_WEAPON` (`Piercing.h`) and `MAX_VIS`. */
export const MAX_PIERCING_SUIT = 4;
export const MAX_PIERCING_WEAPON = 10;
export const MAX_VIS = 9;

const NULL_ID = 0xffff_ffff;

/** `CItemElem::IsDiceRefineryAble(prop)`: armour and weapons. */
export function isDiceRefineryAble(prop: ClientItem): boolean {
  return (
    prop.item_kind2 === IK2_ARMOR ||
    prop.item_kind2 === IK2_WEAPON_MAGIC ||
    prop.item_kind2 === IK2_WEAPON_DIRECT ||
    prop.item_kind2 === IK2_ARMORETC
  );
}

/** `CItemElem::IsEleRefineryAble(prop)`: suits and weapons. */
export function isEleRefineryAble(prop: ClientItem): boolean {
  return (
    prop.item_kind3 === IK3_SUIT ||
    prop.item_kind2 === IK2_WEAPON_MAGIC ||
    prop.item_kind2 === IK2_WEAPON_DIRECT
  );
}

/**
 * `CItemElem::IsPierceAble(targetKind3)`. With the repack's `__NO_PIERCING` (`features.no_piercing`) the suit / shield / weapon
 * branches are compiled out and only vis pets answer (for a vis item); without it a suit takes cards up to `MAX_PIERCING_SUIT`
 * sockets, a shield or weapon socket cards 2 up to `MAX_PIERCING_WEAPON`.
 */
export function isPierceAble(
  target: InvItem,
  prop: ClientItem,
  cardKind3: number,
  noPiercing: boolean,
): boolean {
  const size = target.cards.length;
  if (!noPiercing) {
    if (prop.item_kind3 === IK3_SUIT) {
      if (size <= MAX_PIERCING_SUIT) {
        if (cardKind3 === NULL_ID || cardKind3 === IK3_SOCKETCARD) return true;
      }
      return false;
    }
    if (
      prop.item_kind3 === IK3_SHIELD ||
      prop.item_kind2 === IK2_WEAPON_DIRECT ||
      prop.item_kind2 === IK2_WEAPON_MAGIC
    ) {
      if (size <= MAX_PIERCING_WEAPON) {
        if (cardKind3 === NULL_ID || cardKind3 === IK3_SOCKETCARD2) return true;
      }
      return false;
    }
  }
  const visPet =
    (prop.item_kind3 === IK3_PET && prop.refer_stat1 === PET_VIS) || target.transformVisPet;
  if (visPet && size <= MAX_VIS) return cardKind3 === NULL_ID || cardKind3 === IK3_VIS;
  return false;
}

/** The rules of the dispatch over the TS ports. */
export function createUpgradeRules(opts: { noPiercing: () => boolean }): UpgradeRules {
  return {
    isPierceAble: (target, prop, cardKind3) =>
      isPierceAble(target, prop, cardKind3, opts.noPiercing()),
    isEleRefineryAble,
    isDiceRefineryAble,
  };
}
