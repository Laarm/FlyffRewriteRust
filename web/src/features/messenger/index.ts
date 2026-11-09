// `features/messenger` (WP-CFEAT-39): the messenger feature - the friend list and its messages, the messenger windows, the instant messages and their popups, the notes, the
// privacy settings and the ignore list in server mode, the friend summon and the rename. The player-data cache that the lists read is `features/common/playerData.ts`.
export { messengerModule } from './messengerModule';
export { messengerOf } from './api';
export type { GuildRosterSource, MessengerFeature, PopupRequest, VisitPorts } from './api';
export { DEFAULT_FRIEND, defineMessengerStore, initialMessengerState, MAX_FRIEND } from './store';
export type {
  ChatSettings,
  FriendRecord,
  MessengerState,
  MessengerStore,
  NoteRecord,
} from './store';
export * from './reducers';
export { ImRegistry, ImSession, MAX_IM_LINES } from './im';
export { messengerSenders } from './senders';
export type { MessengerSenders } from './senders';
export { friendMenuEntries, friendReachable, friendRow, friendRows, guildRowsOf } from './view';
export type { CachedFacts, MenuFacts, RowFacts } from './view';
export { MAX_NAME, MIN_SUMMON_NAME, summonCandidates, summonOutcome } from './summon';
export type { SummonOutcome } from './summon';
export { blockListView, serverOwnsSettings } from './settings';
