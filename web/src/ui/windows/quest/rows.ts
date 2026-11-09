// The pure helpers of the quest windows (WP-UI-53). NOT in a lazy module: `index.ts` re-exports them as values and the feature reads them (a value re-export from a lazy module would be an
// `INEFFECTIVE_DYNAMIC_IMPORT`). The numbers are the original's: the tab kinds `QUEST_KIND_*`, the Heaven Tower costs and rows (`WndField.cpp:22170-22445`), `MakeTextColor`
// (`WndQuest.cpp:2354-2417`).

import {
  QUEST_KIND_EVENT,
  QUEST_KIND_NORMAL,
  QUEST_KIND_REQUEST,
  QUEST_KIND_SCENARIO,
} from '../../../gen/defs/quest';
import {
  TID_GAME_HEAVETOWER_FLOOR01,
  TID_GAME_HEAVETOWER_FLOOR01_MSG,
  TID_GAME_HEAVETOWER_FLOOR02,
  TID_GAME_HEAVETOWER_FLOOR02_MSG,
  TID_GAME_HEAVETOWER_FLOOR03,
  TID_GAME_HEAVETOWER_FLOOR03_MSG,
  TID_GAME_HEAVETOWER_FLOOR04,
  TID_GAME_HEAVETOWER_FLOOR04_MSG,
  TID_GAME_HEAVETOWER_FLOOR05,
  TID_GAME_HEAVETOWER_FLOOR05_MSG,
  TID_GAME_HEAVETOWER_FLOOR06,
  TID_GAME_HEAVETOWER_FLOOR06_MSG,
  TID_GAME_HEAVETOWER_FLOOR07,
  TID_GAME_QUEST_ALL_LIST,
  TID_GAME_QUEST_COMPLETE_LIST,
  TID_GAME_QUEST_CURRENT_LIST,
} from '../../../gen/defs/text';

// ---- the list window ------------------------------------------------------------------------------------------------------------------

/** The quest kinds in tab order (the prop of the kind id gives the title of the tab). */
export const QUEST_TAB_KINDS: readonly number[] = [
  QUEST_KIND_SCENARIO,
  QUEST_KIND_NORMAL,
  QUEST_KIND_REQUEST,
  QUEST_KIND_EVENT,
];
/** The English fallback of a tab title when the kind has no property (decision: the original showed an empty tab). */
export const QUEST_TAB_FALLBACK: readonly string[] = ['Scenario', 'Normal', 'Request', 'Event'];
/** The combo of the list: ALL / CURRENT / COMPLETE (the order of `QuestListGroup`). */
export const QUEST_GROUP_TIDS: readonly number[] = [
  TID_GAME_QUEST_ALL_LIST,
  TID_GAME_QUEST_CURRENT_LIST,
  TID_GAME_QUEST_COMPLETE_LIST,
];
/** `CWndQuest::MAX_CHECK_NUMBER`. */
export const MAX_CHECK_NUMBER = 5;
/** The default group of the list (`CURRENT_QUEST_LIST`). */
export const GROUP_CURRENT = 1;

// ---- the Heaven Tower -------------------------------------------------------------------------------------------------------------------

/** The cost of the floors 1..5 in penya (`m_nCost`, `InitText`). */
export const HEAVEN_TOWER_COSTS: readonly number[] = [10000, 30000, 50000, 70000, 100000];
/** The rows of the list (`m_nVecList`). */
export const HEAVEN_TOWER_FLOOR_TIDS: readonly number[] = [
  TID_GAME_HEAVETOWER_FLOOR01,
  TID_GAME_HEAVETOWER_FLOOR02,
  TID_GAME_HEAVETOWER_FLOOR03,
  TID_GAME_HEAVETOWER_FLOOR04,
  TID_GAME_HEAVETOWER_FLOOR05,
  TID_GAME_HEAVETOWER_FLOOR06,
  TID_GAME_HEAVETOWER_FLOOR07,
];
/** The confirm texts of the rows 0..5 (`m_nMsgList`; row 6 closes). */
export const HEAVEN_TOWER_MSG_TIDS: readonly number[] = [
  TID_GAME_HEAVETOWER_FLOOR01_MSG,
  TID_GAME_HEAVETOWER_FLOOR02_MSG,
  TID_GAME_HEAVETOWER_FLOOR03_MSG,
  TID_GAME_HEAVETOWER_FLOOR04_MSG,
  TID_GAME_HEAVETOWER_FLOOR05_MSG,
  TID_GAME_HEAVETOWER_FLOOR06_MSG,
];
/** The row that closes the window ("Do not Enter."). */
export const HEAVEN_TOWER_CLOSE_ROW = 6;
/** The row without a cost (`m_nChoiceNum == 5`). */
export const HEAVEN_TOWER_FREE_ROW = 5;
/** `MAX_FLOOR_COUNT`: more rows than this leave the list at its layout size. */
export const MAX_FLOOR_COUNT = 7;
/** What the layout's list and text controls grow by (`영어 때문에 창 크기 늘임`: the English texts are wider). */
export const HEAVEN_TOWER_WIDEN = 80;

/** The rows of the list: `"%d. %s"` of `TID_GAME_HEAVETOWER_FLOOR01..07`. */
export function heavenTowerRows(getText: (tid: number) => string): string[] {
  return HEAVEN_TOWER_FLOOR_TIDS.map((tid, i) => `${String(i + 1)}. ${getText(tid)}`);
}

/** The row pitch and the offset of the selection band: `gap = height( "123" ) + 2`, `offset = gap >= 16 ? gap - 16 : 0`. */
export function heavenTowerGap(textHeight: number): { gap: number; offset: number } {
  const gap = textHeight + 2;
  return { gap, offset: gap >= 16 ? gap - 16 : 0 };
}

/** The floor number a row sends (`m_nChoiceNum + 1`), `null` for the rows that send nothing (row 5 is floor 6: KEEP `m_nFloor != 6`; row 6 closes). */
export function heavenTowerFloorOf(row: number): 1 | 2 | 3 | 4 | 5 | null {
  switch (row) {
    case 0:
      return 1;
    case 1:
      return 2;
    case 2:
      return 3;
    case 3:
      return 4;
    case 4:
      return 5;
    default:
      return null;
  }
}

// ---- colours ------------------------------------------------------------------------------------------------------------------------------

/**
 * `MakeTextColor( start, end, current, complete )`: the colour between `start` and `end` at `current / complete` (`D3DCOLOR_ARGB( 255, ... )`); 0 for a zero goal. The rate is computed in 32-bit
 * floats like the original and each channel is clamped to the span.
 */
export function makeTextColor(
  start: number,
  end: number,
  current: number,
  complete: number,
): number {
  if (complete === 0) return 0;
  const channel = (shift: number): number => {
    const s = (start >>> shift) & 0xff;
    const e = (end >>> shift) & 0xff;
    const diff = Math.abs(e - s);
    const rate = Math.fround(Math.fround(current) / Math.fround(complete));
    let step = Math.trunc(Math.fround(rate * diff));
    if (!Number.isFinite(step) || step < 0) step = 0;
    if (step > diff) step = diff;
    if (e - s > 0) return s + step;
    if (e - s < 0) return s - step;
    return s;
  };
  return (0xff000000 | (channel(16) << 16) | (channel(8) << 8) | channel(0)) >>> 0;
}
