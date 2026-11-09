// The lord entries of the NPC menu (Plan/45 §11.2, §28.2; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:5371-5400`): ids 91 State, 92 Tender, 93 Vote, 94 Event, 101 Info and 102 RPInfo of
// `MMI_*`. They send nothing; each replaces an open window of its app (`SAFE_DELETE; new; Initialize`). The popup lists whatever the NPC offers (`MaFl_KINGAIDE`, "Lord Manager": INFO, TENDER,
// VOTE, STATE and EVENT; RPINFO is commented out in `character.inc`, so 102 is reachable through the `npcMenuRun` probe hook only): no client gate and no config key.
//
//   91 MMI_LORD_STATE   the candidates      92 MMI_LORD_TENDER   the bid      93 MMI_LORD_VOTE   the ballot
//   94 MMI_LORD_EVENT   the event window    101 MMI_LORD_INFO    the status   102 MMI_LORD_RPINFO   the returned bids

import {
  MMI_LORD_EVENT,
  MMI_LORD_INFO,
  MMI_LORD_RPINFO,
  MMI_LORD_STATE,
  MMI_LORD_TENDER,
  MMI_LORD_VOTE,
} from '../../../gen/defs/neuz';

/** What the menu actions call: the module's window openers. */
export interface LordMenuActions {
  openState(): void;
  openTender(): void;
  openVote(): void;
  openEvent(): void;
  openInfo(): void;
  openRPInfo(): void;
}

/** The ids of the lord entries in the order of the plan. */
export const LORD_MENU_IDS = [
  MMI_LORD_STATE,
  MMI_LORD_TENDER,
  MMI_LORD_VOTE,
  MMI_LORD_EVENT,
  MMI_LORD_INFO,
  MMI_LORD_RPINFO,
] as const;

/** The action of each `MMI_*` id. */
export function menuTable(a: LordMenuActions): ReadonlyMap<number, () => void> {
  return new Map<number, () => void>([
    [
      MMI_LORD_STATE,
      () => {
        a.openState();
      },
    ],
    [
      MMI_LORD_TENDER,
      () => {
        a.openTender();
      },
    ],
    [
      MMI_LORD_VOTE,
      () => {
        a.openVote();
      },
    ],
    [
      MMI_LORD_EVENT,
      () => {
        a.openEvent();
      },
    ],
    [
      MMI_LORD_INFO,
      () => {
        a.openInfo();
      },
    ],
    [
      MMI_LORD_RPINFO,
      () => {
        a.openRPInfo();
      },
    ],
  ]);
}
