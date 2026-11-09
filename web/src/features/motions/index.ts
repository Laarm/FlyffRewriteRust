// `features/motions` (WP-CFEAT-51): the motion shortcuts, the motion window's data and the emoticon shortcuts.
export { motionsModule, motionsOf, EMOTICON_ATLAS, type MotionsFeature } from './motionsModule';
export {
  ANILOOP,
  animLoopOf,
  MotionExecutor,
  PICKUP_RANGE,
  pointWayAngle,
  type MotionEnv,
  type MotionHandler,
  type MotionRow,
} from './execute';
export { MOTION_CHAT_MS, PendingChat } from './pendingChat';
