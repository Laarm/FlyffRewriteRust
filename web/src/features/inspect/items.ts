// Item records for the windows that show items the character does not own (the boss drops of the teleporter, the equipment of an inspected
// player; WP-CFEAT-29): a plain `InvItem` with the item id and the extras that are known, and the `TooltipItemView` the tooltip composer reads.

import type { ClientItem } from '../../data/clientData';
import type { Piercing } from '../../gen/proto/flyff/v1/item_types_pb';
import { HD_TWO } from '../../gen/defs/attribute';
import { isExpired, NULL_ID, partsOf } from '../inventory/info';
import type { InvItem } from '../inventory/items';
import type { InvItemView } from '../inventory/view';

/** The extras of an inspected item (`EQUIP_INFO_ADD`): the element, the awakening word and the sockets. */
export interface ItemExtras {
  readonly num?: number;
  readonly flag?: number;
  readonly abilityOption?: number;
  readonly itemResist?: number;
  readonly resistAbilityOption?: number;
  readonly randomOpt?: bigint;
  readonly piercing?: Pick<Piercing, 'cardItemIds' | 'ultimateGemIds'> | undefined;
  readonly objId?: number;
}

/** A plain item record: nothing but the id (and the extras). */
export function plainItem(itemId: number, extras: ItemExtras = {}): InvItem {
  return {
    objId: extras.objId ?? 0,
    itemId,
    serial: 0,
    itemText: '',
    num: extras.num ?? 1,
    repairNumber: 0,
    hitPoint: 0,
    repair: 0,
    flag: extras.flag ?? 0,
    abilityOption: extras.abilityOption ?? 0,
    idGuild: 0,
    itemResist: extras.itemResist ?? 0,
    resistAbilityOption: extras.resistAbilityOption ?? 0,
    resistSmItemId: 0,
    cards: extras.piercing ? [...extras.piercing.cardItemIds] : [],
    gems: extras.piercing ? [...extras.piercing.ultimateGemIds] : [],
    visExpireAt: [],
    charged: false,
    randomOpt: extras.randomOpt ?? 0n,
    keepTimeAt: 0,
    pet: undefined,
    transformVisPet: false,
    extra: 0,
  };
}

/** The view of an item for the grid cells and the tooltip composer (`name`, `kind2`, `parts`, the icon and the record). */
export function itemView(
  prop:
    Pick<ClientItem, 'name' | 'item_kind2' | 'parts' | 'icon' | 'pack_max' | 'handed'> | undefined,
  item: InvItem,
  serialKey: string | number,
): InvItemView {
  return {
    name: prop?.name ?? `Item ${String(item.itemId)}`,
    kind2: prop?.item_kind2 ?? 0,
    parts: partsOf(prop),
    serial: serialKey,
    objId: item.objId,
    itemId: item.itemId,
    icon: prop && prop.icon !== '' ? `item/${prop.icon}` : null,
    count: item.num,
    stackable: (prop?.pack_max ?? 1) > 1,
    extra: 0,
    using: false,
    expired: isExpired(item),
    cooldown: null,
    twoHanded: prop?.handed === HD_TWO,
    wearable: prop !== undefined && prop.parts !== NULL_ID,
    item,
  };
}
