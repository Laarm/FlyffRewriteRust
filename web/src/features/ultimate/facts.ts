// The facts the ultimate windows read of a bag item (WP-CFEAT-26): the plain `InvItem` and its prop turned into the `UltimateItem` of the window
// ports (`ui/windows/ultimate/ports.ts`), so that the shells never look into the item table.

import type { ClientItem } from '../../data/clientData';
import {
  IK1_WEAPON,
  IK2_MATERIAL,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  WEAPON_GENERAL,
  WEAPON_ULTIMATE,
  WEAPON_UNIQUE,
} from '../../gen/defs/itemkind';
import type { UltimateItem, WeaponFacts, WeaponTier } from '../../ui/windows/ultimate/ports';
import type { InvItem } from '../inventory/items';
import type { InvItemView } from '../inventory/view';
import { ultimateRole } from './gems';

/** `dwReferStat1` of a weapon as a tier. */
export function weaponTier(referStat1: number): WeaponTier {
  switch (referStat1) {
    case WEAPON_GENERAL:
      return 'general';
    case WEAPON_UNIQUE:
      return 'unique';
    case WEAPON_ULTIMATE:
      return 'ultimate';
    default:
      return 'other';
  }
}

/** The weapon facts of an item whose prop is a weapon (`IK1_WEAPON`), `null` otherwise. */
export function weaponFacts(it: InvItem, prop: ClientItem | undefined): WeaponFacts | null {
  if (prop?.item_kind1 !== IK1_WEAPON) return null;
  return {
    tier: weaponTier(prop.refer_stat1),
    level: prop.limit_level1,
    ao: it.abilityOption,
    directOrMagic: prop.item_kind2 === IK2_WEAPON_DIRECT || prop.item_kind2 === IK2_WEAPON_MAGIC,
    // `GetUltimatePiercingSize()` is the length of the socket list, `GetUltimatePiercingItem( i )` its entries
    sockets: it.gems.length,
    gems: it.gems,
  };
}

/** The `UltimateItem` of a bag item (`view` is the tooltip / icon view of the inventory). */
export function ultimateItem(
  it: InvItem,
  prop: ClientItem | undefined,
  view: InvItemView,
): UltimateItem {
  return {
    view,
    objId: it.objId,
    itemId: it.itemId,
    num: it.num,
    extra: it.extra,
    role: ultimateRole(it.itemId),
    material: prop?.item_kind2 === IK2_MATERIAL,
    weapon: weaponFacts(it, prop),
  };
}
