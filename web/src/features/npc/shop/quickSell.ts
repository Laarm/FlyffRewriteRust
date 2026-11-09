// The quick sale (Plan/45 §12.4, Plan/19 §7.7, `CWndQuickSell::OnInitialUpdate`, `src:_Interface/WndShop.cpp:1030-1090`;
// WP-CFEAT-19): which bag items a quick sale takes, their reservation while the window is open, and the total.

import { II_PET_EGG } from '../../../gen/defs/item';
import { IK1_ARMOR, IK1_WEAPON, IK2_JEWELRY } from '../../../gen/defs/itemkind';
import type { ClientItem } from '../../../data';
import { isReserved } from '../../inventory/info';
import { ITEM_FLAG, type InvItem } from '../../inventory/items';
import type { ItemContainer } from '../../inventory/container';

/**
 * The items of the bag that a quick sale takes: usable (it exists and has a prop), a weapon, armour, jewel or a pet egg, not
 * worn, not in use, not upgraded (`ability_option == 0`) and not already reserved by another window.
 */
export function quickSellCandidates(
  container: ItemContainer<InvItem>,
  prop: (itemId: number) => ClientItem | undefined,
): InvItem[] {
  const out: InvItem[] = [];
  for (let slot = 0; slot < container.indexNum; slot += 1) {
    const it = container.getAtSlot(slot);
    if (!it || it.num < 1) continue;
    const p = prop(it.itemId);
    if (!p) continue;
    if (p.item_kind1 !== IK1_WEAPON && p.item_kind1 !== IK1_ARMOR && p.item_kind2 !== IK2_JEWELRY) {
      if (p.id !== II_PET_EGG) continue;
    }
    if (container.isEquip(it.objId)) continue;
    if ((it.flag & ITEM_FLAG.ISUSING) !== 0) continue;
    if (isReserved(it)) continue;
    if (it.abilityOption > 0) continue;
    out.push(it);
  }
  return out;
}

/** `Σ rules.quickSellUnitPrice` over the candidates: one piece per stack (PL-168: a bound item counts 1). */
export function quickSellTotal(
  items: readonly InvItem[],
  unitPrice: (it: InvItem) => number,
): number {
  let total = 0;
  for (const it of items) total += unitPrice(it);
  return total;
}
