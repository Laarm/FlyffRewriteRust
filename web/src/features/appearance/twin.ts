// The opposite-gender twin of an armour (Plan/19 §12.8, 45 §18.4; WP-CFEAT-15): `CMover::GetTransyItem` of the original
// (`src:_Common/Mover.cpp:3033-3090`) over the item table of the client. The server decides again (`RunItemTransy`: `can_transy` of the rules), this
// only finds the item the window shows and sends as `change_item_id`.
//
// The original walks the table in id order and takes the FIRST item that is an armour of the other sex with the same kinds, job, hands, parts,
// item level, ability range and attack speed, and that is of the same "set-ness": both pieces of a named armour set (`CSetItemFinder`) or neither.

import type { ClientItem } from '../../data/clientData';
import { SEX_FEMALE, SEX_MALE } from '../../gen/defs/define';
import { IK2_ARMOR, IK2_ARMORETC } from '../../gen/defs/itemkind';

const isArmor = (p: ClientItem): boolean =>
  p.item_kind2 === IK2_ARMOR || p.item_kind2 === IK2_ARMORETC;
const hasSex = (p: ClientItem): boolean => p.item_sex === SEX_MALE || p.item_sex === SEX_FEMALE;

/** `GetTransyItem`: the first twin of `from` among `items` (any order, the table order of the original is the id order), or `null`. */
export function findTransyTwin(
  from: ClientItem,
  items: readonly ClientItem[],
  inSet: (itemId: number) => boolean,
): ClientItem | null {
  if (!isArmor(from) || !hasSex(from)) return null;
  const fromInSet = inSet(from.id);
  let best: ClientItem | null = null;
  for (const c of items) {
    if (c.id === from.id || !isArmor(c) || !hasSex(c) || c.item_sex === from.item_sex) continue;
    if (
      from.item_kind1 !== c.item_kind1 ||
      from.item_kind2 !== c.item_kind2 ||
      from.item_kind3 !== c.item_kind3 ||
      from.item_job !== c.item_job ||
      from.handed !== c.handed ||
      from.parts !== c.parts ||
      from.item_lv !== c.item_lv ||
      from.ability_min !== c.ability_min ||
      from.ability_max !== c.ability_max ||
      from.attack_speed !== c.attack_speed
    ) {
      continue;
    }
    if (inSet(c.id) !== fromInSet) continue;
    // the table is walked in id order: the lowest id wins
    if (best === null || c.id < best.id) best = c;
  }
  return best;
}

/** The ids of every piece of every named armour set (`CSetItemFinder::GetSetItemByItemId`). */
export function setMemberIds(
  sets: readonly { readonly elems: readonly { readonly item: number }[] }[],
): ReadonlySet<number> {
  const out = new Set<number>();
  for (const s of sets) for (const e of s.elems) out.add(e.item);
  return out;
}
