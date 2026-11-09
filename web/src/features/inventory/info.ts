// Predicates on an item and its prop that the inventory flows decide on (Plan/45 §9.4; `src:_Common/Item.cpp`): the gates of the
// drop, the delete and the double click. Pure functions over the plain records.

import { II_GEN_TOO_COL_EVENT, II_GEN_TOO_COL_NORMAL } from '../../gen/defs/item';
import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_JEWELRY,
  IK3_CLOAK,
  IK3_QUEST,
} from '../../gen/defs/itemkind';
import type { ClientItem } from '../../data';
import { ITEM_FLAG, type InvItem } from './items';

/** `ItemProp::dwFlag` bits (`IP_TYPE`, `ProjectCmn.h:228-236`). */
export const IP_FLAG = { BINDS: 0x01, UNDESTRUCTABLE: 0x02, EQUIP_BIND: 0x04 } as const;

/** `NULL_ID` as the prop columns carry it. */
export const NULL_ID = 0xffff_ffff;

/** `CItemBase::GetExtra() != 0` (`IsUsingItem`): the item is reserved by a trade, vendor, upgrade or quick-sell window. */
export const isReserved = (it: Pick<InvItem, 'extra'>): boolean => it.extra !== 0;

/** `IsCharged()`: a cash item, or one that carries a commercial element stone. */
export const isCharged = (it: Pick<InvItem, 'charged' | 'resistSmItemId'>): boolean =>
  it.charged || it.resistSmItemId !== 0;

/** `IsFlag(expired)`. */
export const isExpired = (it: Pick<InvItem, 'flag'>): boolean =>
  (it.flag & ITEM_FLAG.EXPIRED) !== 0;

/** `IsFlag(binds)`. */
export const isBound = (it: Pick<InvItem, 'flag'>): boolean => (it.flag & ITEM_FLAG.BINDS) !== 0;

/** `IsUndestructable()`: `IP_FLAG_UNDESTRUCTABLE` of the prop. */
export const isUndestructable = (prop: Pick<ClientItem, 'flag'> | undefined): boolean =>
  prop !== undefined && (prop.flag & IP_FLAG.UNDESTRUCTABLE) !== 0;

/** `IsQuest()` (`IK3_QUEST`). */
export const isQuestItem = (prop: Pick<ClientItem, 'item_kind3'> | undefined): boolean =>
  prop?.item_kind3 === IK3_QUEST;

/** A guild cloak: `IK3_CLOAK` with a guild id on the item. */
export const isGuildCloak = (
  it: Pick<InvItem, 'idGuild'>,
  prop: Pick<ClientItem, 'item_kind3'> | undefined,
): boolean => prop?.item_kind3 === IK3_CLOAK && it.idGuild !== 0;

/** `IsCollector(bRefinable)`: the normal collector, and the event one unless only refinable ones count. */
export const isCollector = (itemId: number, refinable = false): boolean =>
  itemId === II_GEN_TOO_COL_NORMAL || (!refinable && itemId === II_GEN_TOO_COL_EVENT);

/** `IsAccessory()`: jewelry (rings, necklaces, earrings). */
export const isAccessory = (prop: Pick<ClientItem, 'item_kind2'> | undefined): boolean =>
  prop?.item_kind2 === IK2_JEWELRY;

/** `IsArmor()` of the gear that can be upgraded to +20. */
export const isArmorKind = (prop: Pick<ClientItem, 'item_kind2'> | undefined): boolean =>
  prop !== undefined && (prop.item_kind2 === IK2_ARMOR || prop.item_kind2 === IK2_ARMORETC);

/** `dwParts` as the client uses it: `-1` for an item that cannot be worn. */
export const partsOf = (prop: Pick<ClientItem, 'parts'> | undefined): number =>
  prop === undefined || prop.parts === NULL_ID ? -1 : prop.parts | 0;
