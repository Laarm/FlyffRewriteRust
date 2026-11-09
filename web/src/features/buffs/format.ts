// The time texts of the buff icons and tooltips (Plan/45 §12; `CTimeSpan` + `Format`):
//
//   world strip            `CWndWorld::RenderOptBuffTime`: "1day" / "3hr" / "5min" / "42sec" from `TID_PK_LIMIT_*` under the icon
//   status window          `CWndBuffStatus::RenderOptBuffTime`: with `better_buff_display` "42s" / "5m" / "3h" / "2j" in the outlined title
//                          font, else the plain seconds count with a black shadow
//   tooltip lines          "%.2d:%.2d:%.2d:%.2d" with days, "%.2d:%.2d:%.2d" with hours, "%.2d:%.2d"
//   duration list          `TID_TOOLTIP_DATE`: "%d Day(s) %d Hr(s) %d Min(s) %d Sec(s)"

import {
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
  TID_TOOLTIP_DATE,
} from '../../gen/defs/text';
import { sprintf } from '../tooltip/printf';
import { timeSpan, type TimeSpan } from '../tooltip/rich';

export type TextOf = (tid: number) => string;

/** `CTimeSpan( (long)(ms / 1000.0f) )`. */
export function spanOfMs(ms: number): TimeSpan {
  return timeSpan(Math.trunc(Math.max(0, ms) / 1000));
}

/** The world strip's timer: `{ text, dx }` (the seconds sit 2 px further left than the others), `null` for nothing to draw. */
export function worldTimer(ct: TimeSpan, text: TextOf): { text: string; dx: number } {
  if (ct.days >= 1) return { text: sprintf(text(TID_PK_LIMIT_DAY), [ct.days]), dx: 5 };
  if (ct.hours >= 1) return { text: sprintf(text(TID_PK_LIMIT_HOUR), [ct.hours]), dx: 5 };
  if (ct.minutes >= 1) return { text: sprintf(text(TID_PK_LIMIT_MINUTE), [ct.minutes]), dx: 5 };
  return { text: sprintf(text(TID_PK_LIMIT_SECOND), [ct.seconds]), dx: 3 };
}

/** The status window's timer (`__BETTER_BUFF_DISPLAY`): "42s", "5m", "3h", "2j"; `''` when no unit applies (0 s left). */
export function statusTimerBetter(ct: TimeSpan): string {
  const s = ct.totalSeconds;
  const m = ct.totalMinutes;
  const h = ct.totalHours;
  if (s > 0 && s <= 59) return `${String(s)}s`;
  if (m > 0 && m <= 59) return `${String(m)}m`;
  if (h > 0 && h <= 23) return `${String(h)}h`;
  if (h > 23) return `${String(Math.trunc(h / 24))}j`;
  return '';
}

/** The status window's timer without the feature: the remaining seconds. */
export function statusTimerPlain(ct: TimeSpan): string {
  return String(ct.totalSeconds);
}

const two = (n: number): string => String(n).padStart(2, '0');

/** The tooltip time line (without the leading newline): days, then hours, else minutes and seconds. */
export function clockText(ct: TimeSpan): string {
  if (ct.days !== 0)
    return `${two(ct.days)}:${two(ct.hours)}:${two(ct.minutes)}:${two(ct.seconds)}`;
  if (ct.hours >= 1) return `${two(ct.hours)}:${two(ct.minutes)}:${two(ct.seconds)}`;
  return `${two(ct.minutes)}:${two(ct.seconds)}`;
}

/** `TID_TOOLTIP_DATE` with the four fields of a span. */
export function dateText(ct: TimeSpan, text: TextOf): string {
  return sprintf(text(TID_TOOLTIP_DATE), [ct.days, ct.hours, ct.minutes, ct.seconds]);
}

/** The span of a millisecond count (tests). */
export const spanOfMsForTest = spanOfMs;
