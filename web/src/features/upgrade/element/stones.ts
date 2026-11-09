// The element stones of the commercial-element window (`CWndCommercialElem::IsSMItem`, `src:_Interface/WndCommItem.cpp:1370-1470`, Plan/45 §18.3, ref/09a §5.9;
// WP-CFEAT-25): five attack stones (the element of a weapon), five defence stones (the element of a suit) and the one stone that takes the element off.
// The tables are the interface facts of the original; `stoneTarget` answers whether a stone fits an item and why not.

import {
  II_CHR_SYS_SCR_DEEARTHYSTONE,
  II_CHR_SYS_SCR_DEFIREASTONE,
  II_CHR_SYS_SCR_DELIGHTINESTONE,
  II_CHR_SYS_SCR_DEWATEILSTONE,
  II_CHR_SYS_SCR_DEWINDYOSTONE,
  II_CHR_SYS_SCR_EARTHYSTONE,
  II_CHR_SYS_SCR_FIREASTONE,
  II_CHR_SYS_SCR_LIGHTINESTONE,
  II_CHR_SYS_SCR_TINEINEDSTONE,
  II_CHR_SYS_SCR_WATEILSTONE,
  II_CHR_SYS_SCR_WINDYOSTONE,
} from '../../../gen/defs/item';
import { IK2_SYSTEM } from '../../../gen/defs/itemkind';
import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
} from '../../../gen/defs/itemkind';
import { ELEMENT } from '../safeSmelt/rules';

export type StoneKind = 'attack' | 'defense' | 'remove';

export interface Stone {
  readonly kind: StoneKind;
  /** The `SAI79` element the stone is for, `0` for the remover. */
  readonly element: number;
}

const STONES: ReadonlyMap<number, Stone> = new Map([
  [II_CHR_SYS_SCR_FIREASTONE, { kind: 'attack', element: ELEMENT.FIRE }],
  [II_CHR_SYS_SCR_WATEILSTONE, { kind: 'attack', element: ELEMENT.WATER }],
  [II_CHR_SYS_SCR_WINDYOSTONE, { kind: 'attack', element: ELEMENT.WIND }],
  [II_CHR_SYS_SCR_LIGHTINESTONE, { kind: 'attack', element: ELEMENT.ELECTRICITY }],
  [II_CHR_SYS_SCR_EARTHYSTONE, { kind: 'attack', element: ELEMENT.EARTH }],
  [II_CHR_SYS_SCR_DEFIREASTONE, { kind: 'defense', element: ELEMENT.FIRE }],
  [II_CHR_SYS_SCR_DEWATEILSTONE, { kind: 'defense', element: ELEMENT.WATER }],
  [II_CHR_SYS_SCR_DEWINDYOSTONE, { kind: 'defense', element: ELEMENT.WIND }],
  [II_CHR_SYS_SCR_DELIGHTINESTONE, { kind: 'defense', element: ELEMENT.ELECTRICITY }],
  [II_CHR_SYS_SCR_DEEARTHYSTONE, { kind: 'defense', element: ELEMENT.EARTH }],
  [II_CHR_SYS_SCR_TINEINEDSTONE, { kind: 'remove', element: ELEMENT.NONE }],
] as [number, Stone][]);

/** The stone an item id is, `null` for any other item. */
export function stoneOf(itemId: number): Stone | null {
  return STONES.get(itemId) ?? null;
}

/** Why a stone does not fit an item: the text id of the chat line (`TID_GAME_NOTELEMENT`, `TID_GAME_NOTEQUALITEM`, `TID_GAME_NOTEQUALELEM`), or `none` (silent). */
export type StoneVerdict =
  | { readonly ok: true }
  | { readonly ok: false; readonly say: 'notElement' | 'notEqualItem' | 'notEqualElem' | 'none' };

/**
 * `IsSMItem`: does the stone (an item row of kind `IK2_SYSTEM`) fit the target (kind2, element, element level)? Attack stones fit weapons of the stone's
 * element, defence stones suits of it, the remover any item that still has an element level.
 */
export function stoneFits(
  stoneItemId: number,
  stoneKind2: number,
  target: { kind2: number; element: number; elementLevel: number },
): StoneVerdict {
  if (stoneKind2 !== IK2_SYSTEM) return { ok: false, say: 'none' };
  const stone = stoneOf(stoneItemId);
  if (!stone) return { ok: false, say: 'none' };
  if (stone.kind === 'remove') {
    return target.elementLevel <= 0 ? { ok: false, say: 'none' } : { ok: true };
  }
  if (target.element === ELEMENT.NONE) return { ok: false, say: 'notElement' };
  const weapon = target.kind2 === IK2_WEAPON_MAGIC || target.kind2 === IK2_WEAPON_DIRECT;
  const armour = target.kind2 === IK2_ARMOR || target.kind2 === IK2_ARMORETC;
  if (stone.kind === 'attack' && armour) return { ok: false, say: 'notEqualItem' };
  if (stone.kind === 'defense' && weapon) return { ok: false, say: 'notEqualItem' };
  if (target.element !== stone.element) return { ok: false, say: 'notEqualElem' };
  return { ok: true };
}
