// What a `DoEquipNtf` does to the VISUAL equipment of a mover (`CMover::DoEquip` / `DoFakeEquip`, `UpdateParts`; Plan/45 §9.1):
// the entity's `MoverVisualState.equip` map (the renderer hashes it every frame and rebuilds the parts that changed) and the
// ride item. The own character takes the container move first (`state.ts` `applyEquip`); the others only learn the `EquipInfo`.

import { PARTS_RIDE } from '../../../gen/defs/neuz';
import type { EntityRecord, MoverVisualState } from '../../../game/entities';
import { attrOption, type InvItem } from '../items';

/** Puts one part of the visual equipment (`item === null` empties it) and keeps the ride item in step. */
export function setVisualPart(
  visual: MoverVisualState,
  part: number,
  item: { readonly itemId: number; readonly flag: number; readonly option: number } | null,
): void {
  if (item) visual.equip.set(part, { itemId: item.itemId, flag: item.flag, option: item.option });
  else visual.equip.delete(part);
  if (part === PARTS_RIDE) visual.rideItem = item ? item.itemId : null;
}

/** The visual entry of an inventory item (`EQUIP_INFO{dwId, nOption = GetAttrOption(), byFlag}`). */
export function equipInfoOf(it: InvItem): { itemId: number; flag: number; option: number } {
  return { itemId: it.itemId, flag: it.flag, option: attrOption(it) };
}

/** Applies the changed parts of an own equip to the entity's visual state. */
export function applyOwnVisual(
  rec: EntityRecord | undefined,
  changed: readonly { readonly part: number; readonly item: InvItem | null }[],
): void {
  const v = rec?.visual;
  if (!v) return;
  for (const c of changed) setVisualPart(v, c.part, c.item ? equipInfoOf(c.item) : null);
}

/** `UI_FLAG` of a worn item: the flag also goes into the `EquipInfo` of the part (`m_aEquipInfo[nParts].byFlag`). */
export function setVisualFlag(rec: EntityRecord | undefined, part: number, flag: number): void {
  const e = rec?.visual?.equip.get(part);
  if (e && rec?.visual) rec.visual.equip.set(part, { ...e, flag });
}
