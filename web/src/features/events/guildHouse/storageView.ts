// The rows of the storage window from the slice (Plan/45 §28.11; `CWndGuildHousing::RefreshItemList`, `src:_Interface/WndHousing.cpp:659-789`; WP-CFEAT-46): every furniture of the house with the facts of its prop. The slot is the index in the
// list of the house; a row whose item the data does not know is skipped (the original asserted).

import type { ClientDataIndex } from '../../../data';
import type { GhRowView } from '../../../ui/windows/housing/rows';
import { itemIconPath } from '../../common/itemIcon';
import type { GhRow } from './slice';

export function ghRowView(
  slot: number,
  row: GhRow,
  data: ClientDataIndex | null,
): GhRowView | null {
  const prop = data?.item(row.itemId);
  if (!prop) return null;
  return {
    slot,
    itemId: row.itemId,
    name: prop.name,
    description: prop.description,
    icon: itemIconPath(prop) ?? '',
    kind2: prop.item_kind2,
    kind3: prop.item_kind3,
    installed: row.setup,
    expireAt: row.expireAt,
    abilityMin: prop.ability_min,
    ctrlObjId: row.ctrlObjId,
    pos: row.pos,
    angle: row.angle,
  };
}

/** All rows of the house in list order. */
export function ghRowViews(rows: readonly GhRow[], data: ClientDataIndex | null): GhRowView[] {
  const out: GhRowView[] = [];
  rows.forEach((r, i) => {
    const v = ghRowView(i, r, data);
    if (v) out.push(v);
  });
  return out;
}

/** The index of the row that stands for the furniture object `objId` (`FindIndex`), `-1` for none. */
export function slotOfObj(rows: readonly GhRow[], objId: number): number {
  return rows.findIndex((r) => r.ctrlObjId === objId);
}
