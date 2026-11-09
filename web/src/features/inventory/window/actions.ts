// `InventoryActions` (Plan/42 §19.4 D, 45 §9.1, §9.4; WP-CFEAT-10): what the inventory shell calls. The decisions behind each call
// (the double-click dispatch, the drop gates) live in `use.ts` and `drop/`; the sends are `senders.ts`.

import type { Shortcut } from '../../../ui/dnd/shortcut';
import type { InventoryActions, InventoryItem } from '../../../ui/windows/char/ports';
import { dropOnTrash, type DropEnv } from '../drop/drop';
import type { InventorySenders } from '../senders';
import type { InventoryState } from '../state';
import type { ItemUse } from './use';

export interface ActionsEnv {
  readonly use: ItemUse;
  readonly senders: Pick<InventorySenders, 'moveItem' | 'sort' | 'hideCoat' | 'renderMask'>;
  readonly drop: DropEnv;
  state(): InventoryState;
  say(tid: number): void;
  /** `g_Option.m_bViewMask = on`. */
  setViewMask(on: boolean): void;
  /** A shortcut of another window dropped on the bag grid (`WIN_ITEMDROP`, 45 §9.4): the shop, vendor and bank windows plug in. */
  dropIn?(sc: Shortcut, fromApp: number): boolean;
}

export function createInventoryActions(env: ActionsEnv): InventoryActions {
  const record = (item: InventoryItem) => env.state().container.getAtId(item.objId);
  return {
    useItem(item) {
      const it = record(item);
      if (it) env.use.doubleClick(it);
    },
    press(item) {
      const it = record(item);
      return it ? env.use.pressBagItem(it) : true;
    },
    cancelCursor() {
      env.use.cursor.cancel();
    },
    unequip(item, part) {
      const it = record(item);
      if (it) env.use.equip(it, part);
    },
    unequipByDrag(objId, part) {
      // the shortcut carries the object id; a drag without a pressed part sends nothing
      if (part < 0) return;
      const it = env.state().container.getAtId(objId);
      if (it) env.use.useItem(it, 0, part, true);
    },
    equip(item) {
      const it = record(item);
      if (it) env.use.equip(it, -1);
    },
    moveItem(_item, from, to) {
      // dropped over no slot (`to = -1`) or on its own slot: nothing happens
      if (to < 0 || from === to) return;
      env.senders.moveItem(from, to);
    },
    dropIn(sc, fromApp) {
      // the shop, vendor, bank, pocket, mail and guild-bank windows register through `InventoryFeature.registerDropIn`
      return env.dropIn?.(sc, fromApp) ?? false;
    },
    trash(sc: Shortcut) {
      return dropOnTrash(env.drop, sc);
    },
    sort() {
      env.senders.sort();
    },
    hideCoat(index, hide) {
      env.senders.hideCoat(index, hide);
    },
    viewMask(on) {
      env.setViewMask(on);
      env.senders.renderMask(on);
    },
    message(tid) {
      env.say(tid);
    },
  };
}
