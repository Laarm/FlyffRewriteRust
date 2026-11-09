// The pure helpers of the lord windows (WP-UI-54, WP-UI-55; `src:_Interface/WndLord.cpp`): the row strings and their geometry, the percent text of the event window with the original's hack, the
// comma cost, the digits of the deposit edit, the pledge length and the countdown of the information window. They live apart from the window classes because the feature and the tests read
// them and the classes themselves are lazy chunks (a lazy class may be imported by `lazy.ts` only).

import {
  TID_GAME_LORD_STATUS_IS,
  TID_GAME_LORD_STATUS_L1,
  TID_GAME_LORD_STATUS_L2,
  TID_GAME_LORD_STATUS_VA,
} from '../../../gen/defs/text';
import {
  type LordElectionState,
  type LordRateRow,
  MAX_DEPOSIT_DIGITS,
  MAX_PLEDGE_BYTES,
} from './ports';

/** `D3DCOLOR_ARGB( 255, 0, 0, 0 )`: the colour of every drawn row. */
export const ROW_COLOR = 0xff000000;
/** `D3DCOLOR_ARGB( 60, 240, 0, 0 )`: the band of the selected row of the state window. */
export const SELECT_BAND_FILL = 'rgba(240, 0, 0, 0.235)';
/** The rows of the candidate lists (`for( i = 1; i < 11; ++i )`). */
export const LIST_ROWS = 10;
/** The pitch of the rows (`(i-1)*16`). */
export const ROW_PITCH = 16;
/** The y of the first text row below the top of the list rectangle (`pCustom->rect.top + 8`). */
export const TEXT_TOP = 8;
/** The y of the first selection band below the top of the list rectangle (`pCustom->rect.top + 6`). */
export const BAND_TOP = 6;

/** The x of the columns below the left edge of the list rectangle. */
export const STATE_COLS = { name: 15, job: 220, open: 313 } as const;
export const VOTE_COLS = { level: 15, name: 60, job: 220 } as const;

/** The y of row `rank` (1-based) of a list whose rectangle starts at `top`. */
export function textY(top: number, rank: number): number {
  return top + TEXT_TOP + (rank - 1) * ROW_PITCH;
}
export function bandY(top: number, rank: number): number {
  return top + BAND_TOP + (rank - 1) * ROW_PITCH;
}

/** The rank and name of the state window: `"  %d        %s"`, one blank less at rank 10 (the digits are wider). */
export function stateRowText(rank: number, name: string): string {
  return rank === 10 ? `  ${String(rank)}       ${name}` : `  ${String(rank)}        ${name}`;
}

/** The text of the tender window's list: `"  %d            %s \n"` per candidate, one blank less from rank 10. */
export function tenderListText(
  rows: readonly { readonly rank: number; readonly name: string }[],
): string {
  return rows
    .map((r) =>
      r.rank >= 10
        ? `  ${String(r.rank)}           ${r.name} \n`
        : `  ${String(r.rank)}            ${r.name} \n`,
    )
    .join('');
}

/**
 * The percent text of the event window: `nTemp = (int)( 100 * factor ); if( nTemp % 5 ) nTemp += 1; "  %d %c"`. The product is taken in double precision on the f32 value, so 0.35f
 * (0.3499999940...) gives 34 and then 35, and 0.45f gives 44 and 45 (KEEP: an f32 product would give 35 at once and hide the hack).
 */
export function percentText(factor: number): string {
  let n = Math.trunc(100 * Math.fround(factor));
  if (n % 5 !== 0) n += 1;
  return `  ${String(n)} %`;
}

/** The comma form of the cost: groups of three digits from the right (`strTemp.Insert( nLength, ',' )`). */
export function commaCost(n: number): string {
  let s = String(Math.trunc(n));
  let len = s.length;
  while (len - 3 > 0) {
    len -= 3;
    s = `${s.slice(0, len)},${s.slice(len)}`;
  }
  return s;
}

/** The digits of the deposit edit as a number: `atol` of a digit string (FIX: no `DWORD` wrap-around; the edit takes at most 15 digits). */
export function parseDeposit(text: string): number {
  const digits = text.replace(/[^0-9]/g, '').slice(0, MAX_DEPOSIT_DIGITS);
  return digits === '' ? 0 : Number(digits);
}

/** `strTemp.GetLength() > 255` of the pledge, counted in UTF-8 bytes (the original counted MBCS bytes; the server cuts at 255 bytes). */
export function pledgeTooLong(text: string): boolean {
  return new TextEncoder().encode(text).length > MAX_PLEDGE_BYTES;
}

/** The pieces of a `CTimeSpan` of `seconds`: days, hours (0-23), minutes and seconds (0-59). */
export function restTime(seconds: number): {
  days: number;
  hours: number;
  minutes: number;
  seconds: number;
} {
  const s = Math.max(0, Math.floor(seconds));
  return {
    days: Math.floor(s / 86400),
    hours: Math.floor(s / 3600) % 24,
    minutes: Math.floor(s / 60) % 60,
    seconds: s % 60,
  };
}

/** The status text of the information window (`CWndLordInfo::OnDraw`): Ready with a lord `_IS`, else `_VA`; Candidacy `_L1`; Vote `_L2`; Expired `_VA`. */
export function statusTid(state: LordElectionState, hasLord: boolean): number {
  switch (state) {
    case 0:
      return hasLord ? TID_GAME_LORD_STATUS_IS : TID_GAME_LORD_STATUS_VA;
    case 1:
      return TID_GAME_LORD_STATUS_L1;
    case 2:
      return TID_GAME_LORD_STATUS_L2;
    case 3:
      return TID_GAME_LORD_STATUS_VA;
  }
}

/** `"%.2d"`. */
export function pad2(n: number): string {
  return String(Math.trunc(n)).padStart(2, '0');
}

/** The next index of a list of `size` rows stepped by `delta` (1 or -1), wrapping at both ends (`BUTTON1..4` of the event window). */
export function stepIndex(index: number, delta: 1 | -1, size: number): number {
  if (size <= 0) return 0;
  return (((index + delta) % size) + size) % size;
}

/** `GetCost( iEEvent, iIEvent )`: 0 for 0 / 0 and for an index beyond its table, else the sum of both prices. */
export function eventCost(
  exp: readonly LordRateRow[],
  item: readonly LordRateRow[],
  e: number,
  i: number,
): number {
  if (e === 0 && i === 0) return 0;
  const a = exp[e];
  const b = item[i];
  if (!a || !b) return 0;
  return a.cost + b.cost;
}
