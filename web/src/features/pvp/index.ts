// `features/pvp` (WP-CFEAT-12, WP-CFEAT-36): the death and revival dialog (M3); the duel windows and messages, the resurrection offer and the
// PvP option (M5). The DPS meter of the same work package lives in `features/hud/dps/`.
export { GC_PARTICIPANT, revivalModule, revivalOf, revivalViewOf } from './revival/revivalModule';
export type { RevivalFeature } from './revival/revivalModule';
export { sendResurrectionCancel, sendRevival, sendRevivalToLodestar } from './revival/senders';
export { definePvpStore, initialPvpState, NULL_ID } from './store';
export type { DpsEntry, DuelCountdown, DuelRequestInfo, PvpState, PvpStore } from './store';
export { duelModule, duelOf } from './duel/duelModule';
export type { DuelFeature } from './duel/duelModule';
export { DuelBook, NO_DUEL } from './duel/book';
export type { DuelFields } from './duel/book';
export { resurrectionModule, resurrectionOf } from './resurrection/resurrectionModule';
export type { ResurrectionFeature } from './resurrection/resurrectionModule';
export {
  acceptsPvp,
  FREEPK_MODE,
  MODE_FLAG,
  PVPCONFIRM_MODE,
  pvpOptionFlag,
  pvpOptionModule,
  pvpOptionOf,
} from './option/pvpOption';
export type { PvpOptionFeature, PvpOptionView } from './option/pvpOption';
