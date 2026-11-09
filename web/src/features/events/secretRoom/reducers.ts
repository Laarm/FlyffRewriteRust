// The reducers of the secret room (Plan/45 §28.6; WP-CFEAT-52; `src:Neuz/DPClient.cpp:17518-17845`): one pure function per server message,
// `reduce<Message>( state, message, env ) -> { state, effects }`. EVERY message is applied whether or not a window is open (`pl_124_secret_room_lists_parsed_without_window`): the lists land in
// `eventStore.secretRoom`, a window that opens later reads them. What is done to the screen is data (`SecretRoomEffect`), carried out by the module: captions, the HUD timer and the windows.
//
//   SecretRoomMngState{state, remain_ms}   `state` is stored; 3 (LINEUP): the master gets the timer READY_TIME, everybody the caption MSG_SELECTION (the timer of anybody else is NOT touched); 4 (ENTRANCE):
//                                          caption MSG_ENTRANCE and the question box, the timer is NOT touched (KEEP); 5 (WARWAIT) START_TIME + MSG_WARWAIT; 103 (WAR) WAR_TIME + MSG_START;
//                                          104 (CLOSEWAIT) CLOSE_TIME + MSG_END; every other value clears the timer of the secret room and shows no caption
//   SecretRoomInfo{continent, sub}         a continent other than 15 / 240 is ignored (FIX: the legacy `find()->second` on `end()`); GUILDLIST replaces the guilds AND the totals (FIX: the legacy
//                                          `map::insert` kept the first totals of the session) and sets the continent; KILLCOUNT and WARSTATE need that continent and an index in the list
//   SecretRoomTenderOpenWndNtf             the bid window; SecretRoomLineUpOpenWndNtf the line-up editor; SecretRoomTenderViewNtf the status window

import {
  SecretRoomInfoSchema,
  type SecretRoomInfo,
  type SecretRoomInfo_GuildList,
  type SecretRoomInfo_KillCount,
  type SecretRoomInfo_WarState,
  type SecretRoomLineUpOpenWndNtf,
  type SecretRoomMngState,
  type SecretRoomTenderOpenWndNtf,
  type SecretRoomTenderViewNtf,
} from '../../../gen/proto/flyff/v1/secret_room_pb';
import {
  TID_GAME_SECRETROOM_CLOSE_TIME,
  TID_GAME_SECRETROOM_GUILD_FAILED,
  TID_GAME_SECRETROOM_MSG_END,
  TID_GAME_SECRETROOM_MSG_ENTRANCE,
  TID_GAME_SECRETROOM_MSG_SELECTION,
  TID_GAME_SECRETROOM_MSG_START,
  TID_GAME_SECRETROOM_MSG_WARWAIT,
  TID_GAME_SECRETROOM_READY_TIME,
  TID_GAME_SECRETROOM_START_TIME,
  TID_GAME_SECRETROOM_TELEPORT_MSG,
  TID_GAME_SECRETROOM_WAR_TIME,
} from '../../../gen/defs/text';
import { type EventState, type SecretRoomGuild, type SecretRoomSlice } from '../store';

/** The manager and continent states of `SecretRoomMngState.state` (`SecretRoom.h:16-17`). */
export const SR_STATE = {
  Close: 1,
  Open: 2,
  LineUp: 3,
  Entrance: 4,
  WarWait: 5,
  War: 6,
  ContClose: 101,
  ContOpen: 102,
  ContWar: 103,
  ContCloseWait: 104,
} as const;

export { SR_WAR } from './constants';
import { SR_WAR } from './constants';

/** The continents of a `SecretRoomInfo` (`CONT_EAST`, `CONT_WEST`). */
export const SR_CONT_EAST = 15;
export const SR_CONT_WEST = 240;

/** The most guilds a continent's list may hold (the shipped `MaxGuildNum`). */
export const SR_MAX_GUILDS = 8;

/** What a reducer reads besides the state. */
export interface SecretRoomEnv {
  /** The own character is the master of his guild (`m_nMemberLv == GUD_MASTER`). */
  readonly isMaster: boolean;
  /** The name of any known guild, `null` when unknown. */
  guildName(id: number): string | null;
}

/** The windows the secret room opens (the arguments are the data of the message; the module builds the request around them). */
export type SecretRoomOpen =
  | { readonly window: 'offer'; readonly min: number; readonly req: number }
  | {
      readonly window: 'selection';
      readonly min: number;
      readonly max: number;
      readonly ids: readonly number[];
    }
  | {
      readonly window: 'offerState';
      readonly myTender: number;
      readonly rank: number;
      readonly secondsToNext: number;
      readonly names: readonly string[];
    }
  | { readonly window: 'infoBox'; readonly tid: number };

export type SecretRoomEffect =
  /** `g_Caption1.AddCaption( GetText( tid ) )` with the SMALL caption font (`m_pFontAPICaption`). */
  | { readonly kind: 'caption'; readonly tid: number; readonly args: readonly (string | number)[] }
  /** `m_szGuildCombatStr = GetText( tid ); m_dwGuildCombatTime = now + ms`. */
  | { readonly kind: 'hudTimer'; readonly tid: number; readonly ms: number }
  /** The timer of the secret room is gone (`m_dwGuildCombatTime = 0xffffffff` of another value). */
  | { readonly kind: 'clearHudTimer' }
  | ({ readonly kind: 'open' } & SecretRoomOpen);

export interface Reduced {
  readonly state: EventState;
  readonly effects: readonly SecretRoomEffect[];
}

const same = (state: EventState): Reduced => ({ state, effects: [] });

const withSr = (
  state: EventState,
  patch: Partial<SecretRoomSlice>,
  effects: readonly SecretRoomEffect[] = [],
): Reduced => ({ state: { ...state, secretRoom: { ...state.secretRoom, ...patch } }, effects });

const caption = (tid: number, ...args: (string | number)[]): SecretRoomEffect => ({
  kind: 'caption',
  tid,
  args,
});
const timer = (tid: number, ms: number): SecretRoomEffect => ({ kind: 'hudTimer', tid, ms });

// ---- SecretRoomMngState -------------------------------------------------------------------------------------------------------------

/** `SECRETROOM_MNG_STATE`. */
export function reduceMngState(
  state: EventState,
  m: SecretRoomMngState,
  env: SecretRoomEnv,
): Reduced {
  const effects: SecretRoomEffect[] = [];
  switch (m.state) {
    case SR_STATE.LineUp:
      // only the master of the guild runs the line-up: the others keep whatever timer they have
      if (env.isMaster) effects.push(timer(TID_GAME_SECRETROOM_READY_TIME, m.remainMs));
      effects.push(caption(TID_GAME_SECRETROOM_MSG_SELECTION));
      break;
    case SR_STATE.Entrance:
      // KEEP: the timer is not touched (the legacy `bTimeSet = FALSE` without clearing)
      effects.push(caption(TID_GAME_SECRETROOM_MSG_ENTRANCE));
      effects.push({ kind: 'open', window: 'infoBox', tid: TID_GAME_SECRETROOM_TELEPORT_MSG });
      break;
    case SR_STATE.WarWait:
      effects.push(timer(TID_GAME_SECRETROOM_START_TIME, m.remainMs));
      effects.push(caption(TID_GAME_SECRETROOM_MSG_WARWAIT));
      break;
    case SR_STATE.ContWar:
      effects.push(timer(TID_GAME_SECRETROOM_WAR_TIME, m.remainMs));
      effects.push(caption(TID_GAME_SECRETROOM_MSG_START));
      break;
    case SR_STATE.ContCloseWait:
      effects.push(timer(TID_GAME_SECRETROOM_CLOSE_TIME, m.remainMs));
      effects.push(caption(TID_GAME_SECRETROOM_MSG_END));
      break;
    default:
      // 1, 2, 6, 101, 102 and the unknown values: the timer of the secret room goes, nothing is said
      effects.push({ kind: 'clearHudTimer' });
      break;
  }
  return withSr(state, { state: m.state }, effects);
}

// ---- SecretRoomInfo -------------------------------------------------------------------------------------------------------------------

/** `SECRETROOM_GUILDLIST`: the guilds of the continent (the receiver's own line-up only) and the kills needed per war state. */
export function reduceGuildList(
  state: EventState,
  continent: number,
  m: SecretRoomInfo_GuildList,
): Reduced {
  const guilds: SecretRoomGuild[] = m.guilds.map((g) => ({
    guildId: g.idGuild,
    war: g.warState,
    kills: g.killCount,
    lineup: [...g.lineup],
  }));
  const totals = new Map<number, number>(m.monsterTotals.map((t) => [t.warState, t.total]));
  return withSr(state, { continent, guilds, totals });
}

/** An index of the list of the continent the client holds, else `null`. */
function indexOf(state: EventState, continent: number, index: number): number | null {
  const sr = state.secretRoom;
  if (continent !== sr.continent) return null;
  return Number.isInteger(index) && index >= 0 && index < sr.guilds.length ? index : null;
}

/** `SECRETROOM_KILLCOUNT`: the kills of one guild. */
export function reduceKillCount(
  state: EventState,
  continent: number,
  m: SecretRoomInfo_KillCount,
): Reduced {
  const i = indexOf(state, continent, m.index);
  if (i === null) return same(state);
  const guilds = state.secretRoom.guilds.map((g, k) => (k === i ? { ...g, kills: m.kills } : g));
  return withSr(state, { guilds });
}

/** `SECRETROOM_WARSTATE`: the war state of one guild (and its kills back to 0); FAILED says who dropped out. */
export function reduceWarState(
  state: EventState,
  continent: number,
  m: SecretRoomInfo_WarState,
  env: SecretRoomEnv,
): Reduced {
  const i = indexOf(state, continent, m.index);
  if (i === null) return same(state);
  const guild = state.secretRoom.guilds[i];
  const guilds = state.secretRoom.guilds.map((g, k) =>
    k === i ? { ...g, war: m.warState, kills: 0 } : g,
  );
  const effects: SecretRoomEffect[] = [];
  if (m.warState === SR_WAR.Failed && guild) {
    const name = env.guildName(guild.guildId);
    if (name !== null) effects.push(caption(TID_GAME_SECRETROOM_GUILD_FAILED, name));
  }
  return withSr(state, { guilds }, effects);
}

/** The sub cases of `SecretRoomInfo` and their reducers (`every_secret_room_info_case_is_handled` compares the keys with the descriptor). */
export const SR_INFO_REDUCERS = {
  guildList: (s: EventState, c: number, m: SecretRoomInfo_GuildList) => reduceGuildList(s, c, m),
  killCount: (s: EventState, c: number, m: SecretRoomInfo_KillCount) => reduceKillCount(s, c, m),
  warState: (s: EventState, c: number, m: SecretRoomInfo_WarState, e: SecretRoomEnv) =>
    reduceWarState(s, c, m, e),
} as const;

/** The sub cases of the oneof, read from the descriptor. */
export function secretRoomInfoCases(): string[] {
  return (SecretRoomInfoSchema.oneofs[0]?.fields ?? []).map((f) => f.localName);
}

/** `SECRETROOM_INFO`. */
export function reduceInfo(state: EventState, m: SecretRoomInfo, env: SecretRoomEnv): Reduced {
  // a continent other than East and West is ignored (FIX of the legacy `find()->second` on `end()`)
  if (m.continent !== SR_CONT_EAST && m.continent !== SR_CONT_WEST) return same(state);
  const sub = m.sub;
  switch (sub.case) {
    case 'guildList':
      return SR_INFO_REDUCERS.guildList(state, m.continent, sub.value);
    case 'killCount':
      return SR_INFO_REDUCERS.killCount(state, m.continent, sub.value);
    case 'warState':
      return SR_INFO_REDUCERS.warState(state, m.continent, sub.value, env);
    case undefined:
      return same(state);
  }
}

// ---- the windows -----------------------------------------------------------------------------------------------------------------------

/** `SECRETROOM_TENDEROPENWND`: the bid window (the preset and the comparison value are the guild's bid, or the minimum without one). */
export function reduceTenderOpenWnd(state: EventState, m: SecretRoomTenderOpenWndNtf): Reduced {
  return {
    state,
    effects: [{ kind: 'open', window: 'offer', min: m.minPenya, req: m.myTender }],
  };
}

/** `SECRETROOM_LINEUPOPENWND`: the line-up editor. */
export function reduceLineUpOpenWnd(state: EventState, m: SecretRoomLineUpOpenWndNtf): Reduced {
  return {
    state,
    effects: [
      { kind: 'open', window: 'selection', min: m.minMembers, max: m.maxMembers, ids: [...m.ids] },
    ],
  };
}

/** `SECRETROOM_TENDERVIEW`: the status window; the rows are numbered over the KNOWN guilds only (a deleted guild takes no number). */
export function reduceTenderView(
  state: EventState,
  m: SecretRoomTenderViewNtf,
  env: SecretRoomEnv,
): Reduced {
  const names: string[] = [];
  for (const id of m.guilds) {
    const name = env.guildName(id);
    if (name !== null) names.push(name);
  }
  return {
    state,
    effects: [
      {
        kind: 'open',
        window: 'offerState',
        myTender: m.myPenya,
        rank: m.myRank,
        secondsToNext: m.remainS,
        names,
      },
    ],
  };
}
