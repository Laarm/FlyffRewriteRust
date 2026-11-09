// The reducers of the messenger messages (Plan/45 §24.1, §8.3; ref/09a §5.11; WP-CFEAT-39): one pure function per legacy handler of `CDPClient` (`OnFriendGameJoin` ...
// `OnTagResult`, `src:Neuz/DPClient.cpp:7932-8481`), taking the messenger mirror and the plain message and returning the new mirror plus the effects the module carries out
// (lines, boxes, cache queries, a friend-state request, the IM windows). No store, no window, no clock: the tests are tables of messages and expected effects.

import {
  TID_DIAG_0059,
  TID_DIAG_0060,
  TID_DIAG_0061,
  TID_GAME_MESSFULLNOTE,
  TID_GAME_MESSNEWNOTE,
  TID_GAME_MESSSENTNOET,
  TID_GAME_MSGDENYINVATE,
} from '../../gen/defs/text';
import {
  type ChatSettings,
  DEFAULT_FRIEND,
  type FriendRecord,
  FRS,
  MAX_FRIEND,
  type MessengerState,
  type NoteRecord,
} from './store';

export type MessengerEffect =
  /** `g_WndMng.PutString( text( tid, args ) )` in the colour of the text. */
  | { readonly kind: 'line'; readonly tid: number; readonly args: readonly string[] }
  /** `g_WndMng.OpenMessageBox( text( tid, args ) )`. */
  | { readonly kind: 'box'; readonly tid: number; readonly args: readonly string[] }
  /** `GetPlayerData( id, TRUE )`; `name` primes the placeholder when the message carried the name. */
  | { readonly kind: 'need'; readonly id: number; readonly name?: string }
  /** The list form (`QueryPlayerData2`). */
  | { readonly kind: 'needAll'; readonly ids: readonly number[] }
  /** `SendGetFriendState()`. */
  | { readonly kind: 'getState' }
  /** The IM window of a friend follows the block state. */
  | { readonly kind: 'imBlocked'; readonly id: number; readonly blocked: boolean }
  /** `<name> logged in`: the popup of a friend who came online (the server applied the privacy option). */
  | { readonly kind: 'popupJoin'; readonly id: number };

export interface Reduced {
  readonly state: MessengerState;
  readonly effects: readonly MessengerEffect[];
}

const same = (state: MessengerState): Reduced => ({ state, effects: [] });

/** `FRS_*` 0..11 (`MAX_FRIENDSTAT` 12); anything else is not a state. */
const validState = (n: number): boolean => Number.isInteger(n) && n >= 0 && n < 12;

/** A copy of the friend map with one entry replaced; `null` when the friend is not on the list. */
function withFriend(
  s: MessengerState,
  id: number,
  patch: (f: FriendRecord) => FriendRecord,
): MessengerState | null {
  const f = s.friends.get(id);
  if (!f) return null;
  const next = new Map(s.friends);
  next.set(id, patch(f));
  return { ...s, friends: next };
}

// ---- the list ------------------------------------------------------------------------------------------------------------------

export interface FriendIn {
  readonly id: number;
  readonly blocked: boolean;
  readonly state: number;
}

/**
 * `OnFriendGameJoin` (`AddFriendGameJoin`): the list and the own state are replaced (`CRTMessenger::Serialize` clears first; more than `MAX_FRIEND` entries load none), the
 * client is connected, the cache is asked for every friend and the states of the friends are requested when there are any.
 */
export function onAddFriendGameJoin(
  s: MessengerState,
  m: { readonly myState: number; readonly friends: readonly FriendIn[] },
): Reduced {
  const friends = new Map<number, FriendRecord>();
  if (m.friends.length <= MAX_FRIEND) {
    for (const f of m.friends) {
      if (f.id !== 0) friends.set(f.id, { blocked: f.blocked, state: f.state });
    }
  }
  const ids = [...friends.keys()];
  const effects: MessengerEffect[] = [];
  if (ids.length > 0) effects.push({ kind: 'needAll', ids }, { kind: 'getState' });
  return { state: { ...s, connected: true, myState: m.myState, friends }, effects };
}

/** `OnAddFriend` (`AddFriendNtf`): `SetFriend( id, NULL )` - a default entry (state 0, not blocked), the name from the cache, the states again. */
export function onAddFriendNtf(
  s: MessengerState,
  m: { readonly idPlayer: number; readonly name: string },
): Reduced {
  if (m.idPlayer === 0) return same(s);
  const friends = new Map(s.friends);
  friends.set(m.idPlayer, DEFAULT_FRIEND);
  return {
    state: { ...s, friends },
    effects: [{ kind: 'need', id: m.idPlayer, name: m.name }, { kind: 'getState' }],
  };
}

/** `OnRemoveFriend` (`RemoveFriendNtf`). */
export function onRemoveFriendNtf(s: MessengerState, id: number): Reduced {
  if (!s.friends.has(id)) return same(s);
  const friends = new Map(s.friends);
  friends.delete(id);
  return { state: { ...s, friends }, effects: [] };
}

/** `OnRemoveFriendState` (`RemoveFriendState`): `SetFriend( id, NULL )` - the entry is reset (and made when it was not there). */
export function onRemoveFriendState(s: MessengerState, id: number): Reduced {
  if (id === 0) return same(s);
  const friends = new Map(s.friends);
  friends.set(id, DEFAULT_FRIEND);
  return { state: { ...s, friends }, effects: [] };
}

/** `OnGetFriendState` (`GetFriendStateNtf`): the state of every friend of both lists (the channel is the cache's business). */
export function onGetFriendState(
  s: MessengerState,
  m: {
    readonly friends: readonly { readonly id: number; readonly state: number }[];
    readonly blocked: readonly { readonly id: number; readonly state: number }[];
  },
): Reduced {
  let friends: Map<number, FriendRecord> | null = null;
  for (const e of [...m.friends, ...m.blocked]) {
    const cur = (friends ?? s.friends).get(e.id);
    if (!cur || !validState(e.state) || cur.state === e.state) continue;
    friends ??= new Map(s.friends);
    friends.set(e.id, { ...cur, state: e.state });
  }
  return friends ? { state: { ...s, friends }, effects: [] } : same(s);
}

/** `OnSetFriendState` (`SetFriendStateNtf`): the own state, or the state of a friend. */
export function onSetFriendState(
  s: MessengerState,
  m: { readonly idPlayer: number; readonly state: number },
  selfId: number,
): Reduced {
  if (!validState(m.state)) return same(s);
  if (m.idPlayer === selfId) return { state: { ...s, myState: m.state }, effects: [] };
  return { state: withFriend(s, m.idPlayer, (f) => ({ ...f, state: m.state })) ?? s, effects: [] };
}

/**
 * `OnFriendIntercept` (`FriendInterceptStateNtf`): the own block of a friend sets his flag (and disables his IM window); a block of the friend on me shows him offline
 * (`pFriend->dwState = FRS_OFFLINE`).
 */
export function onFriendIntercept(
  s: MessengerState,
  m: { readonly idPlayer: number; readonly idFriend: number },
  selfId: number,
): Reduced {
  if (m.idPlayer === selfId) {
    const next = withFriend(s, m.idFriend, (f) => ({ ...f, blocked: true }));
    return {
      state: next ?? s,
      effects: [{ kind: 'imBlocked', id: m.idFriend, blocked: true }],
    };
  }
  return {
    state: withFriend(s, m.idPlayer, (f) => ({ ...f, state: FRS.OFFLINE })) ?? s,
    effects: [],
  };
}

/** `OnFriendNoIntercept` (`FriendNoIntercept`): the block ends and the friend takes the state the server sent (FIX: V15 ignored it). */
export function onFriendNoIntercept(
  s: MessengerState,
  m: { readonly idFriend: number; readonly state: number },
): Reduced {
  const next = withFriend(s, m.idFriend, (f) => ({
    blocked: false,
    state: validState(m.state) ? m.state : f.state,
  }));
  return { state: next ?? s, effects: [{ kind: 'imBlocked', id: m.idFriend, blocked: false }] };
}

/** `OnFriendJoin` (`AddFriendJoin`): a friend logged in (or was unblocked): his state, and the popup "<name> logged in" unless he is offline. */
export function onAddFriendJoin(
  s: MessengerState,
  m: { readonly idFriend: number; readonly state: number },
): Reduced {
  const next = withFriend(s, m.idFriend, (f) => ({ ...f, state: m.state }));
  if (!next) return same(s);
  return {
    state: next,
    effects: m.state !== FRS.OFFLINE ? [{ kind: 'popupJoin', id: m.idFriend }] : [],
  };
}

/** `OnFriendLogOut` (`AddFriendLogout`). */
export function onAddFriendLogout(s: MessengerState, id: number): Reduced {
  return { state: withFriend(s, id, (f) => ({ ...f, state: FRS.OFFLINE })) ?? s, effects: [] };
}

// ---- requests and errors -------------------------------------------------------------------------------------------------------------------------

/** `OnAddFriendCancel` (`AddFriendCancelNtf`). */
export function onAddFriendCancel(s: MessengerState): Reduced {
  return { state: s, effects: [{ kind: 'line', tid: TID_GAME_MSGDENYINVATE, args: [] }] };
}

/** `OnAddFriendError` (`AddFriendError`): 1 the player is a friend already, 2 there is no such player; the box names him. */
export function onAddFriendError(s: MessengerState, m: { error: number; name: string }): Reduced {
  const tid = m.error === 1 ? TID_DIAG_0059 : m.error === 2 ? TID_DIAG_0060 : 0;
  // the original opened a box with an empty text for any other code; nothing is shown here
  return { state: s, effects: tid === 0 ? [] : [{ kind: 'box', tid, args: [m.name] }] };
}

/** `OnAddFriendNotConnect` (`AddFriendNameNotFound`). */
export function onAddFriendNameNotFound(s: MessengerState, name: string): Reduced {
  return { state: s, effects: [{ kind: 'box', tid: TID_DIAG_0061, args: [name] }] };
}

// ---- notes ----------------------------------------------------------------------------------------------------------------------------------

/** `OnTag` (`Tag`): the notes REPLACE the list (the server replays them on a re-attach), they are unread again, and a line says how many there are. */
export function onTag(s: MessengerState, notes: readonly NoteRecord[]): Reduced {
  return {
    state: { ...s, notes, tagChecked: false },
    effects:
      notes.length > 0
        ? [{ kind: 'line', tid: TID_GAME_MESSNEWNOTE, args: [String(notes.length)] }]
        : [],
  };
}

/** `OnTagResult` (`InsertTagResult`): sent, or the mailbox of the friend is full / he is blocked. */
export function onInsertTagResult(s: MessengerState, ok: boolean): Reduced {
  return {
    state: s,
    effects: [{ kind: 'line', tid: ok ? TID_GAME_MESSSENTNOET : TID_GAME_MESSFULLNOTE, args: [] }],
  };
}

/** The notes window was opened (`m_bCheckTag = TRUE`). */
export function onNotesChecked(s: MessengerState): Reduced {
  return s.tagChecked ? same(s) : { state: { ...s, tagChecked: true }, effects: [] };
}

// ---- the server's privacy settings ---------------------------------------------------------------------------------------------------------------------

/** `ChatSettingsNtf`: from now on the client filters nothing itself. */
export function onChatSettings(s: MessengerState, settings: ChatSettings): Reduced {
  return { state: { ...s, settings }, effects: [] };
}

// ---- the guild roster and the presence overlay ---------------------------------------------------------------------------------------------------------

/** `Guild` (the full record): the roster is the members, the cache is asked for their names. */
export function onGuild(
  s: MessengerState,
  m: { readonly idGuild: number; readonly members: readonly number[] },
): Reduced {
  const ids = [...new Set(m.members)];
  return {
    state: { ...s, guildId: m.idGuild, guild: ids },
    effects: ids.length > 0 ? [{ kind: 'needAll', ids }] : [],
  };
}

/** `SetGuild` for the own character: 0 leaves the guild (roster and overlay go), a new guild waits for its `Guild`. */
export function onSetGuild(s: MessengerState, idGuild: number): Reduced {
  if (idGuild === s.guildId) return same(s);
  return {
    state: { ...s, guildId: idGuild, guild: [], presence: new Map() },
    effects: [],
  };
}

/** `AddGuildMemberNtf` of the own guild. */
export function onAddGuildMember(
  s: MessengerState,
  m: { readonly idGuild: number; readonly idPlayer: number; readonly name: string },
): Reduced {
  if (s.guildId === 0 || m.idGuild !== s.guildId || s.guild.includes(m.idPlayer)) return same(s);
  return {
    state: { ...s, guild: [...s.guild, m.idPlayer] },
    effects: [{ kind: 'need', id: m.idPlayer, name: m.name }],
  };
}

/** `RemoveGuildMemberNtf` of the own guild. */
export function onRemoveGuildMember(
  s: MessengerState,
  m: { readonly idGuild: number; readonly idPlayer: number },
): Reduced {
  if (m.idGuild !== s.guildId || !s.guild.includes(m.idPlayer)) return same(s);
  const presence = new Map(s.presence);
  presence.delete(m.idPlayer);
  return {
    state: { ...s, guild: s.guild.filter((id) => id !== m.idPlayer), presence },
    effects: [],
  };
}

/** `DestroyGuildNtf`: the own guild is gone. */
export function onDestroyGuild(s: MessengerState, idGuild: number): Reduced {
  if (idGuild !== s.guildId || s.guildId === 0) return same(s);
  return { state: { ...s, guildId: 0, guild: [], presence: new Map() }, effects: [] };
}

/** `GuildGameJoin` (the online members at the login): the overlay is exactly these. */
export function onGuildGameJoin(
  s: MessengerState,
  m: { readonly ids: readonly number[]; readonly multis: readonly number[] },
): Reduced {
  const presence = new Map<number, number>();
  m.ids.forEach((id, i) => {
    const multi = m.multis[i] ?? 1;
    if (multi > 0) presence.set(id, multi);
  });
  return { state: { ...s, presence }, effects: [] };
}

/** `GuildGameLogin`: one member came online (`login`) or went offline. */
export function onGuildGameLogin(
  s: MessengerState,
  m: { readonly login: boolean; readonly idPlayer: number; readonly multiNo: number },
): Reduced {
  const presence = new Map(s.presence);
  if (m.login && m.multiNo > 0) presence.set(m.idPlayer, m.multiNo);
  else if (m.login) presence.set(m.idPlayer, 1);
  else presence.delete(m.idPlayer);
  return { state: { ...s, presence }, effects: [] };
}

/** `PresenceLogout` and an `UpdatePlayerData` with login 0: the member is not online any more. */
export function onPresenceGone(s: MessengerState, id: number): Reduced {
  if (!s.presence.has(id)) return same(s);
  const presence = new Map(s.presence);
  presence.delete(id);
  return { state: { ...s, presence }, effects: [] };
}
