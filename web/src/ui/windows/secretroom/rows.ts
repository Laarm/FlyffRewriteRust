// The pure helpers of the secret-room windows (WP-UI-56, WP-UI-57): the rows of the lists, the cut of names, the countdown and the numbers the original draws with. They live apart from the
// window classes because the service, the features and the tests read them and the classes themselves are lazy chunks (a lazy class may be imported by `lazy.ts` only).

import { head, pad2, timeSpan } from '../siege/format';
import type { SecretRoomHost, SelectionMember } from './ports';
import {
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
} from '../../../gen/defs/text';

export { OFFER_MAX_DIGITS, parseBid, RANK_OUT_COLOR, guildRow, memberRow } from '../siege/rows';

/** `MAX_SECRETROOM_MEMBER`: the member rows of the quick window (`WIDC_STATIC_MEM1 .. 11`). */
export const MAX_SECRETROOM_MEMBER = 11;

/** The own rank above which the status window prints it red (`MaxGuildNum` 8 of the shipped data; the original compared with its own constant 8). */
export const SR_RANK_RED_ABOVE = 8;

/** `GetLength() > n` of the lists: a name of MORE than `n` characters is cut to `n` and gets the dots; a name of exactly `n` is not cut (unlike the siege's `cutName`). */
export function cutOver(name: string, n: number): string {
  const chars = Array.from(name);
  return chars.length > n ? `${chars.slice(0, n).join('')}...` : name;
}

/** One right-list row of the line-up editor: `"No.%d  Lv%.2d\t%.16s %.10s"` (`n` is the number as printed, from 1). */
export function lineupRow(n: number, m: SelectionMember): string {
  return `No.${String(n)}  Lv${pad2(m.level)}\t${head(m.name, 16)} ${head(m.job, 10)}`;
}

/** The text of the countdown for `seconds` left: `"<days> <hours> <minutes> <seconds>"` of the four `TID_PK_LIMIT_*` templates. */
export function countdownText(host: SecretRoomHost, seconds: number): string {
  const t = timeSpan(seconds);
  return [
    host.fmt(TID_PK_LIMIT_DAY, t.days),
    host.fmt(TID_PK_LIMIT_HOUR, t.hours),
    host.fmt(TID_PK_LIMIT_MINUTE, t.minutes),
    host.fmt(TID_PK_LIMIT_SECOND, t.seconds),
  ].join(' ');
}

// ---- the quick window (754) ---------------------------------------------------------------------------------------------------------------------------

/** `SetActiveMember`: the window is `40 + 34 n` high for `n` rows. */
export const quickHeight = (n: number): number => 40 + 34 * n;
/** The mini mode (the title CLOSE button): one row in an 80 px window. */
export const QUICK_MINI_HEIGHT = 80;
/** The member text is cut over 13 characters (`cutOver`). */
export const QUICK_TEXT_CUT = 13;
/** The window is opened at `y = 112 + 48`. */
export const QUICK_Y = 160;
/** `0x60ffff00`: the row of the member clicked last. */
export const QUICK_FOCUS_FILL = 0x60ffff00;
/** The colours of the rows (`0xAARRGGBB`). */
export const QUICK_COLOR = {
  ok: 0xff000000,
  low: 0xffffff00,
  dead: 0xffff0000,
  away: 0xff878787,
  offline: 0xff000000,
  master: 0xff1fb72d,
} as const;

// ---- the tax rates (715) -----------------------------------------------------------------------------------------------------------------------------

/** One step of a rate button as the original clamps it: `+1` is capped at `max`, `-1` is floored at `min` (a preset above `max` therefore stays until the first PLUS). */
export function stepRate(value: number, dir: -1 | 1, min: number, max: number): number {
  return dir === 1 ? Math.min(max, value + 1) : Math.max(min, value - 1);
}

/** `"%d%%"`. */
export const percentText = (n: number): string => `${String(Math.trunc(n))}%`;
