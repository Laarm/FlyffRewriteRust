// The mini-room entries of the NPC menu (Plan/45 §11.2, §28.10; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:5402-5417`): ids 122-124 of `MMI_*`.
//
//   122 MMI_VISIT_MYROOM     `myRoom = true`, `SendHousingVisitRoom( self )`                (MaFl_Charlie)
//   123 MMI_VISIT_FRIEND     `APP_MINIROOM_LIST` (an open one is replaced)                   (MaFl_Charlie)
//   124 MMI_RETURNTO_WORLD   `APP_QUIT_ROOM`                                                (NPC_Door)

import { MMI_RETURNTO_WORLD, MMI_VISIT_FRIEND, MMI_VISIT_MYROOM } from '../../../gen/defs/neuz';

/** What the menu actions call: the feature's senders and window openers. */
export interface HousingMenuActions {
  visitMyRoom(): void;
  openRoomList(): void;
  openQuitRoom(): void;
}

/** The action of each `MMI_*` id (`registerAction( id, () => table.get( id )() )`). */
export function housingMenuTable(a: HousingMenuActions): ReadonlyMap<number, () => void> {
  return new Map<number, () => void>([
    [
      MMI_VISIT_MYROOM,
      () => {
        a.visitMyRoom();
      },
    ],
    [
      MMI_VISIT_FRIEND,
      () => {
        a.openRoomList();
      },
    ],
    [
      MMI_RETURNTO_WORLD,
      () => {
        a.openQuitRoom();
      },
    ],
  ]);
}
