// `features/events` (WP-CFEAT-07, WP-CFEAT-41): the event flags, the HUD timer line and the guild siege (`siege/`).
export { eventOf } from './api';
export type { EventFeature } from './api';
export { eventsModule, registerEvents } from './eventsModule';
export { flagOn, replaceFlags, setFlag } from './flags';
export {
  defineEventStore,
  EVENT_FLAGS,
  initialEventState,
  initialSiege,
  SIEGE_STATE,
  WAR_STATE,
  WORLD_GUILDWAR,
} from './store';
export type {
  EventState,
  EventStore,
  GuildRankEntry,
  HudTimer,
  HudTimerSource,
  RosterGuild,
  SiegeSlice,
  WinGuild,
} from './store';
// ---- lord (ui-lord, M7)
export { initialLord } from './lord/slice';
export type { LordSlice } from './lord/slice';
// ---- secret room and tax (ui-secret-tax, M7)
export { initialSecretRoom, initialTax } from './store';
export type { SecretRoomGuild, SecretRoomSlice, TaxContinentRow, TaxSlice } from './store';
// ---- M7 GC 1:1 and attendance (task ui-minigames)
export { initialAttendance } from './store';
export type { AttendanceSlice } from './store';
export {
  initialGc1to1,
  GC1TO1_ARENA,
  GC1TO1_PLAYER,
  GC1TO1_STATE,
  GC1TO1_WORLDS,
} from './gc1to1/slice';
export type { Gc1to1Limits, Gc1to1Slice } from './gc1to1/slice';
// ---- rainbow race and quiz (ui-rainbow)
export { initialRainbow } from './rainbow/state';
export type { RainbowSlice } from './rainbow/state';
export { initialQuiz } from './quiz/state';
export type { QuizSlice } from './quiz/state';
