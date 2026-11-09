// The furniture menu of the guild house (`CWndWorld::ShowCCtrlMenu`, `src:_Interface/WndWorld.cpp:11675-11712`, `CWndMenu::OnCommand`, `src:_Interface/WndControl.cpp:3707-3734`; Plan/45 §28.11; WP-CFEAT-46): a click on a guild-house
// furniture opens the mover menu at the top-right corner of its bound rect with three entries. There is NO power check (the server refuses, KEEP).
//
//   MMI_GHOUSE_INFO       `TID_APP_INFOMATION`: opens the storage window in the furniture section when it is closed (nothing when it is open)
//   MMI_GHOUSE_REINSTALL  `GuildHouse->Reset()`: the guild placement in RESET mode with the row's position and angle
//   MMI_GHOUSE_RECALL     `GuildHouse->Remove()`: `GuildHousePacketReq{ REMOVE, slot, row }`; the selection and the mode are cleared

import { MMI_GHOUSE_INFO, MMI_GHOUSE_RECALL, MMI_GHOUSE_REINSTALL } from '../../../gen/defs/neuz';
import {
  TID_APP_INFOMATION,
  TID_MMI_GHOUSE_RECALL,
  TID_MMI_GHOUSE_REINSTALL,
} from '../../../gen/defs/text';

export interface CtrlMenuItem {
  readonly id: number;
  readonly text: string;
}

/** The three entries (texts of the bundle). */
export function ctrlMenuItems(text: (tid: number) => string): CtrlMenuItem[] {
  return [
    { id: MMI_GHOUSE_INFO, text: text(TID_APP_INFOMATION) },
    { id: MMI_GHOUSE_REINSTALL, text: text(TID_MMI_GHOUSE_REINSTALL) },
    { id: MMI_GHOUSE_RECALL, text: text(TID_MMI_GHOUSE_RECALL) },
  ];
}

export interface CtrlMenuActions {
  /** Opens the storage window in the furniture section when it is closed. */
  info(): void;
  /** The placement in RESET mode of the row at `slot`. */
  reinstall(slot: number): void;
  /** The REMOVE request of the row at `slot`; selection and mode are cleared. */
  recall(slot: number): void;
}

/** The action of the picked entry (`slot` = the row the menu was opened for). */
export function runCtrlMenu(id: number, slot: number, a: CtrlMenuActions): boolean {
  switch (id) {
    case MMI_GHOUSE_INFO:
      a.info();
      return true;
    case MMI_GHOUSE_REINSTALL:
      a.reinstall(slot);
      return true;
    case MMI_GHOUSE_RECALL:
      a.recall(slot);
      return true;
    default:
      return false;
  }
}
