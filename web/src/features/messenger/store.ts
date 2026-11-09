// `messengerStore` (Plan/45 §2.3, §24.1; WP-CFEAT-39): the mirror of the friend list and the messenger state of the original (`CRTMessenger`, `g_WndMng.m_RTMessenger`) with the
// pieces of the client that hang on it: the notes of the join and whether the player looked at them (`g_Neuz.m_nTagCount`, `m_bCheckTag`), the privacy toggles and the
// ignore list of the server (`ChatSettingsNtf`, doc 23 §6.11), the minimal roster of the own guild (the ids; names and levels come from the player-data cache) and the
// presence overlay of the guild members (`GuildGameJoin`, `GuildGameLogin`). Names, levels and jobs of friends never live here: `features/common/playerData.ts` has them.
// Scope `session`: the list is replaced by every `AddFriendGameJoin` (login and every world change), the store is reset with the next login.

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';

/** `CRTMessenger::Serialize`: at most this many friends are loaded (`nSize > MAX_FRIEND` loads none). */
export const MAX_FRIEND = 200;

/** `FRS_*` (`messenger.h`); re-exported from the window ports. */
export { FRS } from '../../ui/windows/messenger/ports';

/** `Friend` of `rtmessenger.h`. */
export interface FriendRecord {
  readonly blocked: boolean;
  /** `FRS_*`. */
  readonly state: number;
}

/** The record a new friend starts with (`Friend::Friend()`: no block, state 0 = online). */
export const DEFAULT_FRIEND: FriendRecord = { blocked: false, state: 0 };

/** One undelivered note (`Tag`). */
export interface NoteRecord {
  readonly from: string;
  /** `YYYYMMDD`. */
  readonly date: number;
  readonly text: string;
}

/** `ChatSettingsNtf` (doc 23 §6.11): `true` = accept. */
export interface ChatSettings {
  readonly trade: boolean;
  readonly whisper: boolean;
  readonly party: boolean;
  readonly shout: boolean;
  readonly messenger: boolean;
  readonly messengerJoin: boolean;
  /** `chat.ignore`, in stored order. */
  readonly ignored: readonly string[];
}

export interface MessengerState {
  /** `m_bConnect`: `AddFriendGameJoin` arrived. */
  readonly connected: boolean;
  /** `m_RTMessenger.GetState()`. */
  readonly myState: number;
  /** The friends in the order of the server (character ids ascending). */
  readonly friends: ReadonlyMap<number, FriendRecord>;
  /** The notes of the last `Tag` (a replayed `Tag` replaces them). */
  readonly notes: readonly NoteRecord[];
  /** `m_bCheckTag`: the notes window was opened since the notes arrived. */
  readonly tagChecked: boolean;
  /** The server's privacy toggles and ignore list; `null` until the first `ChatSettingsNtf` (the client then filters nothing itself). */
  readonly settings: ChatSettings | null;
  /** The own guild (`Guild` / `SetGuild`), 0 = none. */
  readonly guildId: number;
  /** The character ids of the own guild. */
  readonly guild: readonly number[];
  /** The presence overlay: a guild member with a channel number > 0 is online (`GuildGameJoin`, `GuildGameLogin`). */
  readonly presence: ReadonlyMap<number, number>;
}

export function initialMessengerState(): MessengerState {
  return {
    connected: false,
    myState: 0,
    friends: new Map(),
    notes: [],
    tagChecked: false,
    settings: null,
    guildId: 0,
    guild: [],
    presence: new Map(),
  };
}

export type MessengerStore = StoreApi<MessengerState>;

/** Defines (or returns) the `messenger` store of a registry. */
export function defineMessengerStore(registry: StoreRegistry): MessengerStore {
  const existing = registry.get('messenger');
  if (existing) return existing as unknown as MessengerStore;
  return registry.defineStore<MessengerState>({
    name: 'messenger',
    scope: 'session',
    owner: '45',
    initial: initialMessengerState,
    fixtures: {
      friends: {
        connected: true,
        friends: new Map([
          [2, { blocked: false, state: 0 }],
          [3, { blocked: true, state: 1 }],
        ]),
      },
    },
  });
}
