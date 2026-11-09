// How long a mail is kept (`CMail::GetMailInfo`, `src:_Common/post.cpp:61-87`; the inbox list `CWndPostReceive::OnDraw` and `CWndPostRead::MailReceiveItem`;
// Plan/19 §11.4, PL-073): from the FULL timestamps (the original subtracted day-of-month fields, which is wrong across a month end). Pure functions.

import { MAX_KEEP_BASIC_DAY, MAX_KEEP_MAX_DAY } from '../../ui/windows/mail/ports';

export interface KeepInfo {
  /** `nKeepingDay`: `(int)( (MAX_KEEP_MAX_DAY + 1) - totalDays )`, the days left plus one (KEEP-QUIRK: days shown as days + 1). */
  readonly days: number;
  /** `dwKeepingTime`: `(MAX_KEEP_MAX_DAY * 24) - totalHours`, the hours left (0 when over). */
  readonly hours: number;
  /** Elapsed hours since the mail was written. */
  readonly elapsedHours: number;
}

export function keepInfo(createdAtSec: number, nowSec: number): KeepInfo {
  const elapsed = Math.max(nowSec - createdAtSec, 0);
  const totalDays = elapsed / 86_400;
  const totalHours = elapsed / 3_600;
  return {
    days: Math.trunc(MAX_KEEP_MAX_DAY + 1 - totalDays),
    hours: Math.max(0, Math.trunc(MAX_KEEP_MAX_DAY * 24 - totalHours)),
    elapsedHours: totalHours,
  };
}

/** `(MAX_KEEP_MAX_DAY * 24) - dwTime > (MAX_KEEP_BASIC_DAY * 24)`: the free keep time (7 days) is over, taking the item costs the custody fee. */
export function inCustody(info: KeepInfo): boolean {
  return info.elapsedHours > MAX_KEEP_BASIC_DAY * 24;
}

/** The text of the time left: hours when one day or less is left (`TID_PK_LIMIT_HOUR`), else days (`TID_PK_LIMIT_DAY`). */
export function keepText(info: KeepInfo, hourTemplate: string, dayTemplate: string): string {
  return info.days <= 1
    ? hourTemplate.replace(/%\.?\d*d/, String(info.hours))
    : dayTemplate.replace(/%\.?\d*d/, String(info.days));
}

/**
 * `D3DXVec2Lerp((255, 0), (0, 150), nDay / 7)` -> `D3DCOLOR_XRGB( (int)x, (int)y, 0 )`: red while the days run out, green when many are left. The
 * macro masks each channel to a byte, so more than 7 days left wraps (14 days: `f = 2`, `x = -255`, `y = 300` -> (1, 44, 0), a dark green).
 */
export function keepColor(days: number): number {
  const f = days / MAX_KEEP_BASIC_DAY;
  const x = Math.trunc(255 + (0 - 255) * f);
  const y = Math.trunc(0 + (150 - 0) * f);
  return (0xff000000 | ((x & 0xff) << 16) | ((y & 0xff) << 8)) >>> 0;
}
