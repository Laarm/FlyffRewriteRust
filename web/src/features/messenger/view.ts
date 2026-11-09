// The read models of the messenger windows (Plan/45 §24.1, §2.8; WP-CFEAT-39): the friend list, the guild roster and the player-data cache become the rows of 42's
// `MessengerView`. Pure: the module gathers the facts, so the rows (names and levels from the cache, the channel, the guild presence) and the entries of the friend menu are tested
// without a client.

import {
  TID_APP_MESSAGE,
  TID_FRS_BLOCK,
  TID_FRS_BLOCKRESTORE,
  TID_FRS_DELETE,
  TID_FRS_MOVE2,
  TID_GAME_ALLOW_VISIT,
  TID_GAME_PROHIBIT_VISIT,
  TID_GAME_TAGSEND,
  TID_MMI_INVITE_PARTY,
} from '../../gen/defs/text';
import {
  FRIEND_MENU,
  type FriendMenuEntry,
  FRS,
  type MessengerRow,
} from '../../ui/windows/messenger/ports';
import type { MessengerState } from './store';

/** The cached data of one character (`PlayerDataEntry`, trimmed to what the rows read). */
export interface CachedFacts {
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly online: boolean;
}

export interface RowFacts {
  readonly cached: (idPlayer: number) => CachedFacts | undefined;
  /** `prj.m_aJob[ job ]`: the job type (`JTYPE_*`) and name; the data may not know the job. */
  readonly job: (job: number) => { readonly type: number; readonly name: string };
  /** Housing (M7): the friend may visit; `undefined` = no housing feature, no colour and no menu entry. */
  readonly visitAllowed?: (idPlayer: number) => boolean;
}

/** The row of a friend: the friend record with the cache's name, level and job. Offline friends show channel 100, online ones the single channel. */
export function friendRow(
  id: number,
  state: number,
  blocked: boolean,
  facts: RowFacts,
): MessengerRow {
  const c = facts.cached(id);
  const job = c?.job ?? 0;
  const info = facts.job(job);
  const visit = facts.visitAllowed?.(id);
  return {
    id,
    name: c?.name ?? '',
    level: c?.level ?? 1,
    job,
    jobType: info.type,
    jobName: info.name,
    state,
    // there is one channel in this build: `uLogin` of an online character is 1, an offline friend shows 100
    channel: state === FRS.OFFLINE ? 100 : 1,
    blocked,
    ...(visit === undefined ? {} : { visitAllowed: visit }),
  };
}

/** The friend page: one row per friend in the order of the list. */
export function friendRows(s: MessengerState, facts: RowFacts): MessengerRow[] {
  return [...s.friends].map(([id, f]) => friendRow(id, f.state, f.blocked, facts));
}

/**
 * The guild page: one row per member of the roster. A member is online when the presence overlay says so (`GuildGameJoin`, `GuildGameLogin`) or the cache does
 * (`uLogin > 0`); the state is ONLINE or OFFLINE, the channel the overlay's number, else 1, else 0 (an offline member draws no digit).
 */
export function guildRowsOf(
  ids: readonly number[],
  presence: ReadonlyMap<number, number>,
  facts: RowFacts,
): MessengerRow[] {
  return ids.map((id) => {
    const c = facts.cached(id);
    const multi = presence.get(id) ?? 0;
    const online = multi > 0 || c?.online === true;
    const job = c?.job ?? 0;
    const info = facts.job(job);
    return {
      id,
      name: c?.name ?? '',
      level: c?.level ?? 1,
      job,
      jobType: info.type,
      jobName: info.name,
      state: online ? FRS.ONLINE : FRS.OFFLINE,
      channel: online ? (multi > 0 ? multi : 1) : 0,
      blocked: false,
    };
  });
}

/** Whether a friend can be written to: online and not blocked (`dwState != FRS_OFFLINE && !bBlock`). */
export const friendReachable = (r: Pick<MessengerRow, 'state' | 'blocked'>): boolean =>
  r.state !== FRS.OFFLINE && !r.blocked;

export interface MenuFacts {
  readonly text: (tid: number) => string;
  /** `AUTH_GAMEMASTER` or above (`g_pPlayer->IsAuthHigher( AUTH_GAMEMASTER )`). */
  readonly isGm: boolean;
  /** `g_Party.IsMember( id )`. */
  readonly inParty: (idPlayer: number) => boolean;
  /** Housing (M7): the friend may visit; `undefined` = no housing feature, no entry. */
  readonly visitAllowed?: boolean | undefined;
}

/**
 * The right-click menu of a friend, in the order of `CWndFriendCtrlEx::OnRButtonUp`: Message (reachable friends), Block / Unblock, Delete, then for a reachable friend
 * Move (a GM only) and Invite to party (not for a member), Send note for an offline or blocked friend, and the housing entry when there is housing.
 */
export function friendMenuEntries(
  row: Pick<MessengerRow, 'id' | 'state' | 'blocked'>,
  m: MenuFacts,
): FriendMenuEntry[] {
  const entries: FriendMenuEntry[] = [];
  const reachable = friendReachable(row);
  const add = (id: number, tid: number): void => {
    entries.push({ id, text: m.text(tid) });
  };
  if (reachable) add(FRIEND_MENU.MESSAGE, TID_APP_MESSAGE);
  add(FRIEND_MENU.BLOCK, row.blocked ? TID_FRS_BLOCKRESTORE : TID_FRS_BLOCK);
  add(FRIEND_MENU.DELETE, TID_FRS_DELETE);
  if (reachable) {
    if (m.isGm) add(FRIEND_MENU.MOVE, TID_FRS_MOVE2);
    if (!m.inParty(row.id)) add(FRIEND_MENU.INVITE_PARTY, TID_MMI_INVITE_PARTY);
  } else {
    add(FRIEND_MENU.NOTE, TID_GAME_TAGSEND);
  }
  if (m.visitAllowed !== undefined) {
    if (m.visitAllowed) add(FRIEND_MENU.PROHIBIT_VISIT, TID_GAME_PROHIBIT_VISIT);
    else add(FRIEND_MENU.ALLOW_VISIT, TID_GAME_ALLOW_VISIT);
  }
  return entries;
}
