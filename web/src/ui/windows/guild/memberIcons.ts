// The pictures and pixel positions of the guild member page (`src:_Interface/WndGuildTabMember.cpp:210-345`, Plan/42 §19.4 J; WP-UI-45). The sprites are the 49 of the
// atlas `Icon/icon_PlayerData.inc` (`CWndWorld::m_texPlayerDataIcon`), the same sheet the messenger lists use (`../messenger/icons.ts`: the job marks are reused):
//
//     44      the master crown                     45-48   the stars of the member levels 1..4 (kingpin, captain, supporter, rookie)
//     14-33   the job and tier marks (messenger/icons.ts `jobMarks`)
//
// Pure functions from the facts of a row to sprite indices and positions, so the numbers are goldens of the tests. The original draws in the CLIENT coordinates of the
// page (`sx = 8`, rows from `sy = 32`, 18 px apart); the list control of this port sits at y = 24 of the page (below the five sort buttons) with the first row 8 px
// below its top, so the page coordinates stay the same and the pictures that reach 4 px above a row are not clipped.

import { jobMarks, type JobMarks } from '../messenger/icons';

export { jobMarks, PLAYER_DATA_ATLAS, spriteRef, parseSpriteRef } from '../messenger/icons';

/** `MEMBER_LIST_HEIGHT`. */
export const ROW_PITCH = 18;
/** `MAX_MEMBER_LIST_DRAW`. */
export const VISIBLE_ROWS = 14;
/** The wheel steps three rows per notch (`m_nCurrentList -= 3`). */
export const WHEEL_ROWS = 3;
/** The page y where the list control starts (`WIDC_*` sort buttons end at 24) and the y of its first row inside it (page y 32 = `sy`). */
export const LIST_TOP = 24;
export const ROWS_TOP = 8;
/** `sx`. */
export const SX = 8;

/** The columns, from the left edge of the page. */
export const COL = {
  rank: SX + 14,
  jobType: SX + 64,
  job: SX + 84,
  level: SX + 126,
  name: SX + 168,
  alias: SX + 264,
} as const;

/** The vertical offsets of the pictures against the text (`sy - 4` for the rank marks, `sy - 3` for the job marks). */
export const RANK_DY = -4;
export const JOB_DY = -3;

/** Text colours (`0xAARRGGBB`): the selected row is blue, an online member black, an offline one grey. */
export const TEXT_COLOR = {
  selected: 0xff0000ff,
  online: 0xff000000,
  offline: 0xff909090,
} as const;
/** The tint of the pictures of an offline member (`0xffff6464`). */
export const OFFLINE_TINT = 0xffff6464;

/** `CGuild` text cut: a name of more than ten characters shows ten and "...". */
export function cutMemberName(name: string): string {
  const chars = Array.from(name);
  return chars.length > 10 ? `${chars.slice(0, 10).join('')}...` : name;
}

export interface RankMark {
  readonly sprite: number;
  readonly x: number;
}

/**
 * The rank marks of a member: the master has one crown (sprite 44) at the first x; a member of level 1..4 has `cls + 1` stars of sprite `44 + rank`, 10 px
 * apart, the first at `sx + 14` minus 5 per extra star. A member level outside 0..4 draws nothing.
 */
export function rankMarks(rank: number, cls: number): RankMark[] {
  if (!Number.isInteger(rank) || rank < 0 || rank > 4) return [];
  const copies = Math.max(1, Math.min(3, Math.trunc(cls) + 1));
  let x = COL.rank;
  if (copies === 2) x -= 5;
  else if (copies === 3) x -= 10;
  if (rank === 0) return [{ sprite: 44, x }];
  return Array.from({ length: copies }, (_, j) => ({ sprite: 44 + rank, x: x + j * 10 }));
}

/** The job marks of a row (`jobMarks` of the messenger lists: pro, master tier by level, hero). */
export function memberJobMarks(jobType: number, job: number, level: number): JobMarks {
  return jobMarks(jobType, job, level);
}
