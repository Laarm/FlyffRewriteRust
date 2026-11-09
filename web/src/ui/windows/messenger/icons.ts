// The icons and the pixel positions of the messenger lists (`src:_Interface/WndMessengerCtrl.cpp:227-1050`, `WndMessenger.cpp:420-520`; Plan/42 §19.4 I; WP-UI-43).
// The sprites are the 49 of the atlas `Icon/icon_PlayerData.inc` (`CWndWorld::m_texPlayerDataIcon`):
//
//     0-6    the seven big state icons (32 x 32): the messenger header      27-33  the master marks (levels) and the hero mark
//     7-13   the seven small state icons (16 x 16): the list rows           34-43  the channel digits 1..10
//     14-18  the base jobs (Vagrant .. Magician)                            44-48  the guild classes
//     19-26  the pro jobs (Knight .. Elementor)
//
// Pure functions from the facts of a row to sprite indices and positions, so the numbers are goldens of the tests.

import { JTYPE_HERO, JTYPE_MASTER, JTYPE_PRO } from '../../../gen/defs/job';
import { FRS } from './ports';

/** The atlas of the player-data icons (the key of the `ui-atlases` bundle). */
export const PLAYER_DATA_ATLAS = 'icon/icon_playerdata.inc';
/** The atlas of the toolbar icons; the messenger does not draw from it, it is listed in the bundle check. */
export const MESSENGER_ICON_ATLAS = 'icon/icon_iconmessenger.inc';
/** The mail picture of the header (`Theme/WndMail.dds`, 12 x 12, colour key magenta). */
export const MAIL_PICTURE = 'WndMail.dds';
export const MAIL_PICTURE_SIZE = 12;

/** `dwMyState` of `CWndMessengerEx::OnDraw`: AUTOABSENT shows as ABSENT, ONLINE as 2, OFFLINE as 8, everything else as it is. */
export function mappedState(state: number): number {
  if (state === FRS.AUTOABSENT) return FRS.ABSENT;
  if (state === FRS.ONLINE) return 2;
  if (state === FRS.OFFLINE) return 8;
  return state;
}

/** The big state icon of the header (sprites 0..6), `null` for a state the atlas has no picture of. */
export function bigStateSprite(state: number): number | null {
  const i = mappedState(state) - 2;
  return i >= 0 && i <= 6 ? i : null;
}

/** The small state icon of a row (sprites 7..13), `null` for a state without one. */
export function smallStateSprite(state: number): number | null {
  const i = mappedState(state) - 2;
  return i >= 0 && i <= 6 ? 7 + i : null;
}

/** The channel digit (sprites 34..43 for 1..10); nothing for 0, 100 (offline) and anything outside 1..10. */
export function channelSprite(channel: number): number | null {
  return channel !== 100 && channel > 0 && channel < 11 ? 34 + channel - 1 : null;
}

/** The master mark by level (`nMasterIndex`): below 70 -> 27, then 28 .. 32 per ten levels up to 120; above 120 the original keeps 27. */
export function masterMarkSprite(level: number): number {
  if (level < 70) return 27;
  if (level < 80) return 28;
  if (level < 90) return 29;
  if (level < 100) return 30;
  if (level < 110) return 31;
  if (level <= 120) return 32;
  return 27;
}

/** The job mark of a row: the type mark (master / hero) and the job icon; either may be absent. */
export interface JobMarks {
  readonly type: number | null;
  readonly job: number | null;
}

/** The sprites of the job columns (`JOB_TYPE_ICON_X`, `JOB_ICON_X`): pro `19 + job - 6`, master mark + `19 + job - 16`, hero mark 33 + `19 + job - 24`, else `14 + job`. */
export function jobMarks(jobType: number, job: number, level: number): JobMarks {
  if (jobType === JTYPE_PRO) return { type: null, job: spriteOrNull(19 + job - 6) };
  if (jobType === JTYPE_MASTER) {
    return { type: masterMarkSprite(level), job: spriteOrNull(19 + job - 16) };
  }
  if (jobType === JTYPE_HERO) return { type: 33, job: spriteOrNull(19 + job - 24) };
  return { type: null, job: spriteOrNull(14 + job) };
}

/** A sprite index inside the atlas (49 sprites), else `null`: a job without a picture draws nothing. */
const spriteOrNull = (i: number): number | null => (i >= 0 && i < 49 ? i : null);

/** The sprite reference of a cell: `a:<n>`, `null` for none. */
export const spriteRef = (i: number | null): string | null =>
  i === null ? null : `a:${String(i)}`;
/** The sprite index of a cell reference, `null` for none or a malformed one. */
export function parseSpriteRef(icon: string | null | undefined): number | null {
  if (typeof icon !== 'string' || !icon.startsWith('a:')) return null;
  const n = Number(icon.slice(2));
  return Number.isInteger(n) && n >= 0 ? n : null;
}

// ---- positions (client coordinates of the list control; a row is 20 px high from y = 3) ------------------------------------------

/** `m_nFontHeight`. */
export const ROW_PITCH = 20;
/** `CPoint pt( 3, 3 )`. */
export const ROWS_TOP = 3;
/** The x of the picture / text columns (`__CAMPUS` layout). */
export const COL = {
  channel: 20,
  state: 76,
  level: 127,
  jobType: 174,
  job: 194,
  name: 226,
} as const;
/** The text rows sit 3 px below the row top. */
export const TEXT_DY = 3;
/** The job-name tooltip hit area: `CRect( 188, y, 188 + 24, y + 20 )`. */
export const JOB_TIP_X = 188;
export const JOB_TIP_W = 24;

/** The names are cut at 10 characters (`IsDBCSLeadByte` steps; here: characters) and get "..." (`strFormat.GetLength() > 10`). */
export function cutName(name: string, max = 10): string {
  const chars = Array.from(name);
  return chars.length > max ? `${chars.slice(0, max).join('')}...` : name;
}

/** The server name of the header: at most 18 characters, then "...". */
export function cutServerName(name: string): string {
  return cutName(name, 18);
}

/** Name colours (`0xAARRGGBB`): black, selected, blocked, housing visitor. */
export const NAME_COLOR = {
  normal: 0xff000000,
  selected: 0xff6060ff,
  blocked: 0xffff0000,
  visitor: 0xff00ff00,
} as const;
/** The header texts (`0xff606060`). */
export const HEADER_TEXT_COLOR = 0xff606060;

/** The colour of a name: a housing visitor is green over everything, a blocked friend red over the selection. */
export function nameColor(
  row: { blocked: boolean; visitAllowed?: boolean },
  selected: boolean,
): number {
  if (row.visitAllowed === true) return NAME_COLOR.visitor;
  if (row.blocked) return NAME_COLOR.blocked;
  return selected ? NAME_COLOR.selected : NAME_COLOR.normal;
}

/** The header of the window (`WndMessenger.cpp:420-520`). */
export const HEADER = {
  /** The big state icon. */
  icon: { x: 8, y: 8 },
  /** The own name is drawn twice, one pixel apart (fake bold). */
  name: { x: 47, y: 10 },
  nameBold: { x: 48, y: 10 },
  /** `(TID_FRS_*_STATUS)`. */
  state: { x: 46, y: 27 },
  /** The server name on the friend and guild tabs. */
  server: { x: 170, y: 13 },
  /** The campus points on the campus tab. */
  campus: { x: 190, y: 20 },
} as const;

/** The flash of the mail icon and of the notes button: alpha 50 .. 255, 8 per 60 Hz step. */
export const FLASH = { min: 50, max: 255, step: 8, hz: 60 } as const;

/** Advances the flash by `dtMs` of 60 Hz steps; returns the new counter and direction. */
export function flashStep(
  counter: number,
  dir: 1 | -1,
  dtMs: number,
): { counter: number; dir: 1 | -1 } {
  const steps = Math.max(1, Math.round((dtMs * FLASH.hz) / 1000));
  let c = counter;
  let d: 1 | -1 = dir;
  for (let i = 0; i < steps; i += 1) {
    c += d * FLASH.step;
    if (c >= FLASH.max) {
      c = FLASH.max;
      d = -1;
    }
    if (c <= FLASH.min) {
      c = FLASH.min;
      d = 1;
    }
  }
  return { counter: c, dir: d };
}
