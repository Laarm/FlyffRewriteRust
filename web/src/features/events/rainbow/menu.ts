// The rainbow-race entries of the NPC menu (Plan/45 §11.2, §28.7; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:4628-4712`): ids 107-119 of `MMI_*`.
//
//   107 MMI_LORD_RAINBOWAPPLICATION  `RainbowRaceApplicationOpenWndReq`      108 MMI_LORD_RAINBOWWAIT    the info window 768 (`rainbowraceinfo`)
//   109 MMI_LORD_RAINBOWRULE         the rule window 770 (`rainbowracerule`)    110 MMI_LORD_RAINBOWTOPTEN  `RainbowRacePrevRankingOpenWndReq`
//   111 MMI_LORD_RAINBOWWIN          the prize window 774                       112 MMI_LORD_RAINBOWEND     `RainbowRaceReqFinish`
//   113 + g  MMI_LORD_RAINBOW_<GAME> `RainbowRaceMiniGamePacket{ ext false, packet{ 1 << g, OPENWND 1, 0, 0 } }` for ANY g (KEEP: the original sends it without a client check, the server answers
//            `TID_GAME_RAINBOWRACE_ISNOTNOWMINIGAME`; the "on the current game" guard of Plan/45 §28.7 is deliberately not added)
//
// The coloured NPCs (`MaFl_Rainbow<Colour>`) carry ONE entry each (113 - 119), so a click on them runs it at once (`resolveNpcClick`); the start NPC carries 107-112 (a popup).

import {
  MMI_LORD_RAINBOW_KAWIBAWIBO,
  MMI_LORD_RAINBOW_LADDER,
  MMI_LORD_RAINBOWAPPLICATION,
  MMI_LORD_RAINBOWEND,
  MMI_LORD_RAINBOWRULE,
  MMI_LORD_RAINBOWTOPTEN,
  MMI_LORD_RAINBOWWAIT,
  MMI_LORD_RAINBOWWIN,
} from '../../../gen/defs/neuz';
import { MP } from '../../../ui/windows/rainbow/constants';

/** `g_eLocal` event 25: the rainbow race (`events.rainbow_race`). */
export const EVENT_RAINBOW_RACE = 25;

/** What the menu actions call: the module's senders and window openers. */
export interface RainbowMenuActions {
  applicationOpenWnd(): void;
  openInfo(): void;
  openRule(): void;
  prevRankingOpenWnd(): void;
  openPrize(): void;
  reqFinish(): void;
  /** `SendMinigamePacket( game, MP_OPENWND )`. */
  openGame(game: number, state: number): void;
}

/** The ids 107 - 119. */
export const RAINBOW_MENU_IDS: readonly number[] = Array.from(
  { length: MMI_LORD_RAINBOW_LADDER - MMI_LORD_RAINBOWAPPLICATION + 1 },
  (_, i) => MMI_LORD_RAINBOWAPPLICATION + i,
);

/** The action of each `MMI_*` id; the module wraps each in the flag gate. */
export function rainbowMenuTable(a: RainbowMenuActions): ReadonlyMap<number, () => void> {
  const table = new Map<number, () => void>([
    [
      MMI_LORD_RAINBOWAPPLICATION,
      () => {
        a.applicationOpenWnd();
      },
    ],
    [
      MMI_LORD_RAINBOWWAIT,
      () => {
        a.openInfo();
      },
    ],
    [
      MMI_LORD_RAINBOWRULE,
      () => {
        a.openRule();
      },
    ],
    [
      MMI_LORD_RAINBOWTOPTEN,
      () => {
        a.prevRankingOpenWnd();
      },
    ],
    [
      MMI_LORD_RAINBOWWIN,
      () => {
        a.openPrize();
      },
    ],
    [
      MMI_LORD_RAINBOWEND,
      () => {
        a.reqFinish();
      },
    ],
  ]);
  for (let game = 0; game < 7; game += 1) {
    table.set(MMI_LORD_RAINBOW_KAWIBAWIBO + game, () => {
      a.openGame(game, MP.OPENWND);
    });
  }
  return table;
}
