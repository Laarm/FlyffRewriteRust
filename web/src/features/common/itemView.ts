// The shell-facing view of a plain item record (`InvItemView`): name, icon, count, flags for the item grids of the bank, the pockets, the mail and the
// other windows that show items outside the inventory (Plan/45 §9.1). The inventory's own builder (`inventory/view.ts`) adds the cooldown groups; the
// items of these windows have none. Views keep their identity while the item and the item bundle do not change (the shells cache their icon cells per
// object).

import { HD_TWO } from '../../gen/defs/attribute';
import type { ClientDataIndex } from '../../data';
import { isExpired, NULL_ID, partsOf } from '../inventory/info';
import { ITEM_FLAG, type InvItem } from '../inventory/items';
import type { InvItemView } from '../inventory/view';
import { itemIconPath } from './itemIcon';

/** Builds the views of items of one window; the cache is dropped when the item bundle changes. */
export class ItemViewCache {
  #cache = new WeakMap<InvItem, InvItemView>();
  #dataVersion = -1;

  constructor(
    private readonly data: () => Pick<ClientDataIndex, 'item' | 'version'> | null | undefined,
  ) {}

  view(it: InvItem): InvItemView {
    const data = this.data();
    const v = data?.version ?? -1;
    if (v !== this.#dataVersion) {
      this.#dataVersion = v;
      this.#cache = new WeakMap();
    }
    const hit = this.#cache.get(it);
    if (hit) return hit;
    const prop = data?.item(it.itemId);
    const view: InvItemView = {
      name: prop?.name ?? `Item ${String(it.itemId)}`,
      kind2: prop?.item_kind2 ?? 0,
      parts: partsOf(prop),
      serial: it.serial,
      objId: it.objId,
      itemId: it.itemId,
      icon: itemIconPath(prop, it),
      count: it.num,
      stackable: (prop?.pack_max ?? 1) > 1,
      extra: it.extra,
      using: (it.flag & ITEM_FLAG.ISUSING) !== 0,
      expired: isExpired(it),
      cooldown: null,
      twoHanded: prop?.handed === HD_TWO,
      wearable: prop !== undefined && prop.parts !== NULL_ID,
      item: it,
    };
    this.#cache.set(it, view);
    return view;
  }
}
