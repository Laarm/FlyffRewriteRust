// The secret-room entries of the NPC menu (Plan/45 §11.2, §28.6; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:4488-4560`): ids 84-90, 100 and 103 of `MMI_*`. The two secret-room NPCs are
// placed by the server only when flag 24 was on at the load of the world (45 §28.0), so there is no client gate and no config key.
//
//   84 MMI_SECRET_OFFER           SecretRoomTenderOpenWndReq         85 MMI_SECRET_OFFERSTATE   SecretRoomTenderViewReq
//   86 MMI_SECRET_SELECTION       SecretRoomLineUpOpenWndReq         87 MMI_SECRET_ENTRANCE     resets the digits of the HUD board, closes an open 710, SecretRoomEntrance
//   89 MMI_SECRET_TAXRATES_CHECK  the tax check window               90 MMI_SECRET_BOARD        the board (panel `secretroomboard`)
//   100 MMI_SECRET_ENTRANCE_1     SecretRoomTeleportToDungeon        103 MMI_SECRET_CANCEL      the cancel confirmation
//   88 MMI_SECRET_TAXRATES_CHANGE has no action in V15 (not registered); 97 MMI_HEAVEN_TOWER is `../heavenTower`

import {
  MMI_SECRET_BOARD,
  MMI_SECRET_CANCEL,
  MMI_SECRET_ENTRANCE,
  MMI_SECRET_ENTRANCE_1,
  MMI_SECRET_OFFER,
  MMI_SECRET_OFFERSTATE,
  MMI_SECRET_SELECTION,
  MMI_SECRET_TAXRATES_CHECK,
} from '../../../gen/defs/neuz';

/** The panel of the help bundle of the board (`Client/SecretRoomBoard.inc`, the lower-case stem). */
export const BOARD_PANEL = 'secretroomboard';
/** The panel of the bid window (`Client/SecretRoomDesc.inc`). */
export const DESC_PANEL = 'secretroomdesc';

/** What the menu actions call: the module's senders and window openers. */
export interface SecretRoomMenuActions {
  tenderOpenWnd(): void;
  tenderView(): void;
  lineUpOpenWnd(): void;
  /** Resets the shown digits of the board, closes the question box (710) and sends the entrance. */
  entrance(): void;
  checkTax(): void;
  openBoard(): void;
  teleportToDungeon(): void;
  openCancelConfirm(): void;
}

/** The action of each `MMI_*` id (`registerAction( id, () => actions[ id ]() )`). */
export function menuTable(a: SecretRoomMenuActions): ReadonlyMap<number, () => void> {
  return new Map<number, () => void>([
    [
      MMI_SECRET_OFFER,
      () => {
        a.tenderOpenWnd();
      },
    ],
    [
      MMI_SECRET_OFFERSTATE,
      () => {
        a.tenderView();
      },
    ],
    [
      MMI_SECRET_SELECTION,
      () => {
        a.lineUpOpenWnd();
      },
    ],
    [
      MMI_SECRET_ENTRANCE,
      () => {
        a.entrance();
      },
    ],
    [
      MMI_SECRET_TAXRATES_CHECK,
      () => {
        a.checkTax();
      },
    ],
    [
      MMI_SECRET_BOARD,
      () => {
        a.openBoard();
      },
    ],
    [
      MMI_SECRET_ENTRANCE_1,
      () => {
        a.teleportToDungeon();
      },
    ],
    [
      MMI_SECRET_CANCEL,
      () => {
        a.openCancelConfirm();
      },
    ],
  ]);
}
