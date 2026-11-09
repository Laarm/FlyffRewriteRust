// The quiz entries of the NPC menu (Plan/45 §11.2, §28.8; `CWndWorld::OnCommand`, `src:_Interface/WndWorld.cpp:6253-6283`): ids 137-139 of `MMI_*`. `MaFl_Ete` (Flaris) carries 137 and `MMI_DIALOG`, `MaFl_Tonge` (world 211)
// carries 138, 139 and `MMI_DIALOG`, so both open the popup and the entry is picked from it. No client gate: the quiz is started by a GM / the script and the shipped data has no questions (PD-12).
//
//   137 MMI_QUIZ_ENTRANCE      the confirm `TID_GAME_QUIZ_TELEPORT_QUIZ_IN`, Yes -> `QuizEntrance`
//   138 MMI_QUIZ_TELE_QUIZZONE state OPEN (1) or WATCHINGZONE_OPEN (3) -> `QuizTeleport{ 100 }`, else the chat line `TID_GAME_QUIZ_DO_NOT_TELEPORT`
//   139 MMI_QUIZ_EXIT          the confirm `TID_GAME_QUIZ_TELEPORT_QUIZ_OUT`, Yes -> `QuizTeleport{ 106 }`

import { MMI_QUIZ_ENTRANCE, MMI_QUIZ_EXIT, MMI_QUIZ_TELE_QUIZZONE } from '../../../gen/defs/neuz';

export interface QuizMenuActions {
  confirmEntrance(): void;
  teleportQuizZone(): void;
  confirmExit(): void;
}

export const QUIZ_MENU_IDS: readonly number[] = [
  MMI_QUIZ_ENTRANCE,
  MMI_QUIZ_TELE_QUIZZONE,
  MMI_QUIZ_EXIT,
];

export function quizMenuTable(a: QuizMenuActions): ReadonlyMap<number, () => void> {
  return new Map<number, () => void>([
    [
      MMI_QUIZ_ENTRANCE,
      () => {
        a.confirmEntrance();
      },
    ],
    [
      MMI_QUIZ_TELE_QUIZZONE,
      () => {
        a.teleportQuizZone();
      },
    ],
    [
      MMI_QUIZ_EXIT,
      () => {
        a.confirmExit();
      },
    ],
  ]);
}
