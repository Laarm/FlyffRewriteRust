// The equipment of an inspected player (`CWndQueryEquip`, `OnQueryEquip`, `src:Neuz/DPClient.cpp:15835-15860`; Plan/45 §9.8; WP-CFEAT-29):
// the 31 `PARTS_*` entries of the window. The base of each worn part is the mover's visual equipment (`m_aEquipInfo`: item id, flag,
// packed upgrade word); the answer of `QueryEquipReq` adds the rest per part (the awakening word, the sockets, the element). Pure: the module
// reads the entity table and the data.

import type { ClientItem } from '../../data/clientData';
import { MAX_HUMAN_PARTS } from '../../gen/defs/neuz';
import type { Piercing } from '../../gen/proto/flyff/v1/item_types_pb';
import type { InspectItem } from '../../ui/windows/nav/ports';
import { itemView, plainItem } from './items';

/** One `QueryEquipEntry` of the answer. */
export interface EquipExtra {
  readonly part: number;
  readonly randomOptItemId: bigint;
  readonly piercing?: Pick<Piercing, 'cardItemIds' | 'ultimateGemIds'> | undefined;
  readonly itemResist: number;
  readonly resistAbilityOption: number;
}

/** A worn part of the mover (`m_aEquipInfo[part]`). */
export interface WornPart {
  readonly itemId: number;
  readonly flag: number;
  /** `nOption`: `ability | element << 16 | elementLevel << 24`. */
  readonly option: number;
}

/** The first part the loops of the window walk (`for( int i = 2; ...`). */
export const FIRST_PART = 2;

/** The window's 31 entries (parts below 2 and parts without an item are `null`). */
export function buildEquip(
  worn: ReadonlyMap<number, WornPart>,
  extras: readonly EquipExtra[],
  prop: (itemId: number) => ClientItem | undefined,
  tag: string,
): (InspectItem | null)[] {
  const byPart = new Map<number, EquipExtra>();
  for (const e of extras) byPart.set(e.part, e);
  const out: (InspectItem | null)[] = [];
  for (let part = 0; part < MAX_HUMAN_PARTS; part += 1) {
    const w = part < FIRST_PART ? undefined : worn.get(part);
    if (!w || w.itemId === 0xffff_ffff || w.itemId === 0) {
      out.push(null);
      continue;
    }
    const x = byPart.get(part);
    const rec = plainItem(w.itemId, {
      flag: w.flag,
      abilityOption: w.option & 0xff,
      objId: part,
      ...(x
        ? {
            itemResist: x.itemResist,
            resistAbilityOption: x.resistAbilityOption,
            randomOpt: x.randomOptItemId,
            piercing: x.piercing,
          }
        : {}),
    });
    const view = itemView(prop(w.itemId), rec, `${tag}:${String(part)}`);
    out.push({ ...view, part });
  }
  return out;
}
