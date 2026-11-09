// The prize views of the Kawibawibo win window and a plain `InvItem` for items that are not in the bag (the tooltip composer needs an `InvItemView`).

import type { InvItem } from '../../inventory/items';

/** A fresh stack of `num` units of `itemId` (the defaults of `itemFromWire` for an item nobody owns). The serial is `-itemId`: the tooltip cache keys by serial and real serials are positive. */
export function plainItem(itemId: number, num: number): InvItem {
  return {
    objId: 0,
    itemId,
    serial: -itemId,
    itemText: '',
    num,
    repairNumber: 0,
    hitPoint: -1,
    repair: 0,
    flag: 0,
    abilityOption: 0,
    idGuild: 0,
    itemResist: 0,
    resistAbilityOption: 0,
    resistSmItemId: 0,
    cards: [],
    gems: [],
    visExpireAt: [],
    charged: false,
    randomOpt: 0n,
    keepTimeAt: 0,
    pet: undefined,
    transformVisPet: false,
    extra: 0,
  };
}
