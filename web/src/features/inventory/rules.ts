// The item-prop lookups of the inventory reducers over the client data (`ctx.data.item`, the `items` bundle): the stack cap,
// the part an item is worn in and the `UI_AO` limit of an item. A bundle that is not loaded gives the safe answers (a
// non-stackable, non-wearable item).

import type { ClientDataIndex } from '../../data';
import { isCollector, partsOf } from './info';
import type { ItemRules } from './state';

/** The `ItemRules` of a data index (`null` before the bundles exist). */
export function itemRulesOf(
  data: () => Pick<ClientDataIndex, 'item'> | null | undefined,
  nowSec: () => number,
): ItemRules {
  const prop = (id: number) => data()?.item(id);
  return {
    packMax: (id) => prop(id)?.pack_max ?? 1,
    parts: (id) => partsOf(prop(id)),
    updateEnv: {
      // `UpdateItem`: +20 for gear and accessories (`__EXT_ENCHANT`), +5 for a refinable collector
      aoMax: (it) => (isCollector(it.itemId, true) ? 5 : 20),
      nowSec,
    },
  };
}
