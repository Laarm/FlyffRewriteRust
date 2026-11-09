// The one mapping from an item (and its pet record) to its icon file (`CItemBase::SetTexture`, Item.cpp:80-117; BFIX1 #017). Every
// window that shows an item INSTANCE builds its icon here: the seven hatched pet items (`IK3_EGG` other than `II_PET_EGG`) ship the
// files `<icon>_00/_01/_02.dds` only, picked by the pet's level (`petItemIconFile`), every other item is `item/<icon>`.

import type { ClientItem } from '../../data/clientData';
import { IK3_EGG } from '../../gen/defs/itemkind';
import { petItemIconFile } from '../../game/pets/petData';

/**
 * The icon path (`item/<file>`) of an item, `null` for an unknown prop or an empty icon. `item` carries the pet record of an instance
 * (`InvItem`); a prop-only site (a shop stock entry, a reward list) passes none.
 */
export function itemIconPath(
  prop: Pick<ClientItem, 'id' | 'icon' | 'item_kind3'> | undefined,
  item?: { readonly pet?: { readonly level: number } | undefined },
): string | null {
  if (!prop || prop.icon === '') return null;
  if (prop.item_kind3 === IK3_EGG) {
    return `item/${petItemIconFile(prop.icon, prop.id, item?.pet?.level)}`;
  }
  return `item/${prop.icon}`;
}
