// The cheer points of the client (Plan/45 §24.3, Plan/21 §14; WP-CFEAT-40): the tooltip of the Cheer motion in the motion window (`CWndMotion1::OnMouseWndSurface`,
// `src:_Interface/WndMotion.cpp:215-265`) and on the taskbar (`CWndTaskBar::PutTooltip`, `src:_Interface/WndTaskBar.cpp:635-680`). Pure: the clock is a parameter.

import {
  TID_CHEER_MESSAGE1,
  TID_CHEER_MESSAGE2,
  TID_PK_LIMIT_MINUTE,
  TID_PK_LIMIT_SECOND,
} from '../../gen/defs/text';
import { sprintf } from '../tooltip/printf';

/** `MAX_CHEERPOINT`. */
export const MAX_CHEERPOINT = 3;
/** `TICK_CHEERPOINT`: a point comes back every 60 minutes (and a login starts the first period). */
export const CHEER_POINT_MS = 60 * 60_000;

/**
 * The wait for the next point that a join starts with. The join message of this server carries `cheer_rest_ms` 0 (`mover/wire.rs` sends no timer), while the server's own rule is
 * "a login: no points, the first one in 60 minutes" (`social/cheer.rs::on_join`): with fewer than three points and no time in the message the client assumes the 60 minutes, so the tooltip
 * does not show "0sec" for an hour. A message that does carry a time is taken as it is. Drop the assumption when the wire carries the real rest.
 */
export function joinRestMs(points: number, restMs: number): number {
  return restMs > 0 || points >= MAX_CHEERPOINT ? restMs : CHEER_POINT_MS;
}

/** The cheer facts of the own character: the points and when the next one comes (`host.now()` ms, `GetTickCount()` + the rest of `SetCheerParam`). */
export interface CheerState {
  readonly points: number;
  readonly restUntil: number;
}

/**
 * The seconds until the next point. FIX: the original made a `CTimeSpan` of `( m_dwTickCheer - GetTickCount() ) / 1000` in unsigned arithmetic, so a point that was overdue wrapped
 * around to a huge number; the rest never goes below 0 here.
 */
export function cheerRestSeconds(state: CheerState, now: number): number {
  return Math.max(0, Math.floor((state.restUntil - now) / 1000));
}

/**
 * The remaining time as the tooltip prints it: whole minutes (`TID_PK_LIMIT_MINUTE`, `%.1dmin`) when there is at least one, else the seconds (`TID_PK_LIMIT_SECOND`, `%.1dsec`).
 * FIX: the original read the minute and second COMPONENTS of the `CTimeSpan` (`GetMinutes()` is 0..59), so a full hour showed "0sec".
 */
export function cheerRestText(
  state: CheerState,
  now: number,
  text: (tid: number) => string,
): string {
  const rest = cheerRestSeconds(state, now);
  const minutes = Math.floor(rest / 60);
  return minutes >= 1
    ? sprintf(text(TID_PK_LIMIT_MINUTE), [minutes])
    : sprintf(text(TID_PK_LIMIT_SECOND), [rest]);
}

/**
 * The tooltip of the Cheer motion: the description of the motion, a gap (`"\n"` in the motion window, `"\n\n"` on the taskbar), the points line, and while the points are not full a
 * line with the label and the remaining time glued together (the texts carry no separator, KEEP).
 */
export function cheerTooltip(
  description: string,
  state: CheerState,
  now: number,
  text: (tid: number) => string,
  gap: '\n' | '\n\n' = '\n',
): string {
  let out = `${description}${gap}${sprintf(text(TID_CHEER_MESSAGE1), [state.points])}`;
  if (state.points < MAX_CHEERPOINT) {
    out += `\n${text(TID_CHEER_MESSAGE2)}${cheerRestText(state, now, text)}`;
  }
  return out;
}
