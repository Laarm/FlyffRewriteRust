// The mini-game entries of the NPC menu (Plan/45 §11.2; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:5063-5130`): ids 44 `MMI_KAWIBAWIBO`, 45 `MMI_FINDWORD`, 46 `MMI_FIVESYSTEM`, 47
// `MMI_REASSEMBLE`. They send nothing themselves (the windows do when they open). An already open game window answers the chat line `TID_GAME_MINIGAMEUSING` and opens nothing; 45 also
// opens the inventory applet first (the cards come from the bag).

import {
  MMI_FINDWORD,
  MMI_FIVESYSTEM,
  MMI_KAWIBAWIBO,
  MMI_REASSEMBLE,
} from '../../../gen/defs/neuz';

/** What the menu actions call: the module's openers. */
export interface MiniGameMenuActions {
  kawibawibo(): void;
  findWord(): void;
  dice(): void;
  puzzle(): void;
}

/** The ids of the mini-game entries in the order of the plan. */
export const MINIGAME_MENU_IDS = [
  MMI_KAWIBAWIBO,
  MMI_FINDWORD,
  MMI_FIVESYSTEM,
  MMI_REASSEMBLE,
] as const;

/** The action of each `MMI_*` id. */
export function menuTable(a: MiniGameMenuActions): ReadonlyMap<number, () => void> {
  return new Map<number, () => void>([
    [
      MMI_KAWIBAWIBO,
      () => {
        a.kawibawibo();
      },
    ],
    [
      MMI_FINDWORD,
      () => {
        a.findWord();
      },
    ],
    [
      MMI_FIVESYSTEM,
      () => {
        a.dice();
      },
    ],
    [
      MMI_REASSEMBLE,
      () => {
        a.puzzle();
      },
    ],
  ]);
}
