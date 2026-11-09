// The attendance and coupon notices (Plan/45 §28.0, 22 §4.8; WP-CFEAT-45; `src:Neuz/DPClient.cpp:16282-16307, 17458-17475`):
//
//   EventCoupon { coupons }       the coupon event notice: `attendance.coupons = coupons` (the V15 client ignored the count) and the "Mail" popup `TID_GAME_EVENT_COUPON`, five minutes
//   CallTheRoll { attendance_bits }   the attendance flags of the receiver: `attendance.bits` (a bigint, one bit per day) and the "Event" popup `TID_GAME_EVENT_INSTANCE_TITLE( Y, M, D )` of the
//                                 LOCAL date, thirty minutes; a click on the popup closes it ("Mail" ignores clicks). The `CWebBox` page the original opened is dropped.
//
// No window, no send. The shipped data produces no `CallTheRoll` (the 531 window ended in 2009); the coupons come with event-lua-extras' Event.lua override.

import type { ClientContext } from '../../../app/context';
import { TID_GAME_EVENT_COUPON, TID_GAME_EVENT_INSTANCE_TITLE } from '../../../gen/defs/text';
import { wallNow } from '../../../platform/time';
import type { FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { formatTid } from '../../common/text';
import { messengerOf } from '../../messenger/api';
import { eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import type { AttendanceSlice, EventStore } from '../store';

/** The life of the coupon popup (`MIN( 5 )`) and of the attendance popup (`MIN( 30 )`). */
export const COUPON_POPUP_MS = 300_000;
export const ROLL_POPUP_MS = 1_800_000;

/** The key of each popup (`g_WndMng.OpenInstantMsg( key )`). */
export const COUPON_POPUP_KEY = 'Mail';
export const ROLL_POPUP_KEY = 'Event';

/** The public face of the attendance notices of a context. */
export interface AttendanceFeature {
  readonly store: EventStore;
  /** `{ bits, coupons }` now. */
  slice(): AttendanceSlice;
}

/** `EventCoupon`: the slice after the message. */
export function reduceCoupon(s: AttendanceSlice, coupons: number): AttendanceSlice {
  return s.coupons === coupons ? s : { ...s, coupons };
}

/** `CallTheRoll`: the slice after the message. */
export function reduceRoll(s: AttendanceSlice, bits: bigint): AttendanceSlice {
  return s.bits === bits ? s : { ...s, bits };
}

const features = new WeakMap<ClientContext, AttendanceFeature>();

/** The attendance feature of a context; `undefined` before the module registered. */
export function attendanceOf(ctx: ClientContext): AttendanceFeature | undefined {
  return features.get(ctx);
}

/** Registers the two notices (after the events feature: `minigamesM7.ts` registers both under the one handler module `events.minigames`). */
export function registerAttendance(fc: FeatureContext): AttendanceFeature {
  const ctx = fc.app;
  const store = (eventOf(ctx) ?? registerEvents(fc)).store;
  const slice = (): AttendanceSlice => store.getState().attendance;
  const set = (next: AttendanceSlice): void => {
    if (next !== slice()) store.setState({ attendance: next });
  };

  fc.net.on('eventCoupon', (m) => {
    set(reduceCoupon(slice(), m.coupons));
    messengerOf(ctx)?.popup({
      key: COUPON_POPUP_KEY,
      text: fc.texts.text(TID_GAME_EVENT_COUPON),
      ttlMs: COUPON_POPUP_MS,
    });
  });
  fc.net.on('callTheRoll', (m) => {
    set(reduceRoll(slice(), m.attendanceBits));
    // `CTime::GetCurrentTime()`: the local date of the player's computer
    const now = new Date(wallNow());
    messengerOf(ctx)?.popup({
      key: ROLL_POPUP_KEY,
      text: formatTid(fc.texts, TID_GAME_EVENT_INSTANCE_TITLE, [
        now.getFullYear(),
        now.getMonth() + 1,
        now.getDate(),
      ]),
      ttlMs: ROLL_POPUP_MS,
      // a click on the "Event" popup closes it (`WndMessenger.cpp:1039-1068`)
      onClick: () => true,
    });
  });

  const feature: AttendanceFeature = { store, slice };
  features.set(ctx, feature);
  addProbeMember(ctx, 'attendance', () => ({
    bits: slice().bits.toString(),
    coupons: slice().coupons,
  }));
  return feature;
}
