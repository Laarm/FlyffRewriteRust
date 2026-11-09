// `features/events/attendance` (WP-CFEAT-45): the coupon and attendance notices.
export {
  attendanceOf,
  COUPON_POPUP_KEY,
  COUPON_POPUP_MS,
  reduceCoupon,
  reduceRoll,
  registerAttendance,
  ROLL_POPUP_KEY,
  ROLL_POPUP_MS,
} from './attendanceModule';
export type { AttendanceFeature } from './attendanceModule';
