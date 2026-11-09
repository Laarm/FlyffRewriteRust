// The reducers of the siege (Plan/45 §28.4; WP-CFEAT-41; `src:Neuz/DPClient.cpp:6859-7945`): one pure function per `GuildCombat` sub, `reduce<Sub>( state, sub, env ) -> { state, effects }`. EVERY sub is
// applied whether or not a window is open (PL-124 "siege list parsing"): the lists land in `eventStore.siege`, a window that opens later reads them. What is done to the screen is data
// (`SiegeEffect`), carried out by the module: boxes, chat lines, captions, windows, player-data queries, the entity state and the HUD timer.
//
// The gate of a window that depends on the state of the siege (`gcInWindow` while the siege is not closed, `gcSelectPlayer` outside the line-up phase) is decided here, from the state the
// reducer is given: the module only opens what the reducer says.

import type { GuildCombat } from '../../../gen/proto/flyff/v1/guild_siege_pb';
import {
  TID_GAME_GUILDCOMBAT_EXIT_MSG,
  TID_GAME_GUILDCOMBAT_JACKPOT,
  TID_GAME_GUILDCOMBAT_JACKPOT_CANCEL,
  TID_GAME_GUILDCOMBAT_JACKPOT_LIMIT1,
  TID_GAME_GUILDCOMBAT_JACKPOT_LIMIT2,
  TID_GAME_GUILDCOMBAT_JOIN_LIMIT_TIME,
  TID_GAME_GUILDCOMBAT_JOIN_NOW,
  TID_GAME_GUILDCOMBAT_MAINTENANCE,
  TID_GAME_GUILDCOMBAT_MAINTENANCE2,
  TID_GAME_GUILDCOMBAT_MVP_PENYA,
  TID_GAME_GUILDCOMBAT_NOT_MVP,
  TID_GAME_GUILDCOMBAT_NOW_PLAYER_MAKEUP,
  TID_GAME_GUILDCOMBAT_PLAYER_MAKEUP,
  TID_GAME_GUILDCOMBAT_PLAYER_MAKEUP_FINISH,
  TID_GAME_GUILDCOMBAT_REQUESTEND,
  TID_GAME_GUILDCOMBAT_WAR,
  TID_GAME_GUILDCOMBAT_WAR_END,
  TID_GAME_GUILDCOMBAT_WINNER_NOT_GUILD,
  TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP,
} from '../../../gen/defs/text';
import { I32_MAX } from '../../../ui/windows/siege/ports';
import { type EventState, type SiegeSlice, SIEGE_STATE, WAR_STATE, WORLD_GUILDWAR } from '../store';
import { type LogEntry, NOT_GUILD, realtimeLine } from './log';
import type { PlayerPointEntry } from './ranking';

type Sub = NonNullable<GuildCombat['sub']['value']>;
type Case = NonNullable<GuildCombat['sub']['case']>;
/** The message of one case of the oneof. */
type Of<C extends Case> = Extract<GuildCombat['sub'], { case: C }>['value'];

/** What a reducer reads besides the state (`g_pPlayer`, the world, the guild). */
export interface SiegeEnv {
  /** `ctx.now()` in ms. */
  readonly now: number;
  /** The world the character is in. */
  readonly worldId: number;
  readonly selfCharId: number;
  readonly selfObjId: number;
  /** The own character is the master of his guild. */
  readonly isMaster: boolean;
  /** … the master or a kingpin. */
  readonly isMasterOrKingpin: boolean;
  /** `g_pPlayer->GetGuild()->m_szGuild`, `''` without a guild. */
  readonly selfGuildName: string;
  /** `g_pPlayer->GetGold()`. */
  readonly purse: number;
  /** The name of any known guild, `null` when unknown. */
  guildName(id: number): string | null;
}

/** The windows the siege opens (the arguments are the data of the message; the module builds the request around them). */
export type SiegeOpen =
  | {
      readonly window: 'offer';
      readonly min: number;
      readonly myBid: number;
      readonly prize: number;
    }
  | {
      readonly window: 'offerState';
      readonly secondsToNext: number;
      readonly prize: number;
      readonly names: readonly string[];
      readonly myBid: number;
      readonly rank: number;
    }
  | {
      readonly window: 'selection';
      readonly maxJoin: number;
      readonly maxWar: number;
      readonly ids: readonly number[];
      readonly defender: number;
    }
  | { readonly window: 'joinSelection'; readonly map: number; readonly timeoutMs: number }
  | { readonly window: 'infoBox'; readonly text: string }
  | { readonly window: 'infoBox2'; readonly text: string }
  | { readonly window: 'result'; readonly entries: readonly LogEntry[] }
  | { readonly window: 'rankPerson'; readonly entries: readonly PlayerPointEntry[] };

export type SiegeEffect =
  /** `g_WndMng.OpenMessageBox( GetText( tid ) )` with the printf arguments. */
  | { readonly kind: 'box'; readonly tid: number; readonly args: readonly (string | number)[] }
  /** `g_WndMng.PutString( GetText( tid ) )`: a white general chat line. */
  | { readonly kind: 'chat'; readonly tid: number; readonly args: readonly (string | number)[] }
  /** A white general chat line of a text built by the reducer. */
  | { readonly kind: 'chatText'; readonly text: string }
  /** `g_Caption1.AddCaption( GetText( tid ) )`: the big caption. */
  | { readonly kind: 'caption'; readonly tid: number }
  | ({ readonly kind: 'open' } & SiegeOpen)
  /** `SendQueryPlayerData( ids )`. */
  | { readonly kind: 'needAll'; readonly ids: readonly number[] }
  /** `pMover->m_nGuildCombatState = state` (when the mover is in view). */
  | { readonly kind: 'entityState'; readonly objId: number; readonly state: number }
  /** `m_szGuildCombatStr = GetText( tid ); m_dwGuildCombatTime = now + ms`. */
  | { readonly kind: 'hudTimer'; readonly tid: number; readonly ms: number }
  /** `m_dwGuildCombatTime = 0xffffffff`. */
  | { readonly kind: 'clearHudTimer' };

export interface Reduced {
  readonly state: EventState;
  readonly effects: readonly SiegeEffect[];
}

const same = (state: EventState): Reduced => ({ state, effects: [] });

const withSiege = (
  state: EventState,
  patch: Partial<SiegeSlice>,
  effects: readonly SiegeEffect[] = [],
): Reduced => ({ state: { ...state, siege: { ...state.siege, ...patch } }, effects });

const box = (tid: number, ...args: (string | number)[]): SiegeEffect => ({
  kind: 'box',
  tid,
  args,
});
const chat = (tid: number, ...args: (string | number)[]): SiegeEffect => ({
  kind: 'chat',
  tid,
  args,
});

/** The ids that are worth a player-data query (no id twice, none that is 0). */
const ids = (list: Iterable<number>): number[] => [...new Set(list)].filter((id) => id !== 0);

// ---- the subs ---------------------------------------------------------------------------------------------------------------------------

/** `GC_WINGUILD`. */
export function reduceGcWinGuild(state: EventState, m: Of<'gcWinGuild'>): Reduced {
  return withSiege(state, {
    winGuild: { index: m.combatIndex, guildId: m.winGuild, streak: m.winCount },
  });
}

/** `GC_IN_WINDOW`: the bid window, or the box "the siege is closed" while the applications are not open. */
export function reduceGcInWindow(state: EventState, m: Of<'gcInWindow'>): Reduced {
  if (state.siege.state !== SIEGE_STATE.Close) {
    return { state, effects: [box(TID_GAME_GUILDCOMBAT_REQUESTEND)] };
  }
  return {
    state,
    effects: [
      {
        kind: 'open',
        window: 'offer',
        min: m.minBid,
        myBid: m.myBid,
        prize: Number(m.prize),
      },
    ],
  };
}

/** `GC_IN_COMPLETE`: registered, does nothing (its V15 handler is commented out). */
// eslint-disable-next-line @typescript-eslint/no-unused-vars -- the signature of every reducer; the message is empty
export function reduceGcInComplete(state: EventState, _m: Of<'gcInComplete'>): Reduced {
  return same(state);
}

/** `GC_REQUEST_STATUS`: the list of the applying guilds replaces any open state window. */
export function reduceGcRequestStatus(
  state: EventState,
  m: Of<'gcRequestStatus'>,
  env: SiegeEnv,
): Reduced {
  const own = env.selfGuildName.toLowerCase();
  // `stricmp( pGuild->m_szGuild, strGuildName ) == 0`: the first applicant with the own guild's name
  const at = own === '' ? -1 : m.guildNames.findIndex((n) => n.toLowerCase() === own);
  return {
    state,
    effects: [
      {
        kind: 'open',
        window: 'offerState',
        secondsToNext: m.secondsToNext,
        prize: Number(m.prize),
        names: [...m.guildNames],
        myBid: m.myBid,
        rank: at < 0 ? -1 : at + 1,
      },
    ],
  };
}

/** `GC_SELECTPLAYER`: the editor with the stored line-up, or the box "the line-up is made". */
export function reduceGcSelectPlayer(state: EventState, m: Of<'gcSelectPlayer'>): Reduced {
  if (!m.requestWar) return same(state);
  if (!m.window) return { state, effects: [box(TID_GAME_GUILDCOMBAT_PLAYER_MAKEUP)] };
  // the editor opens only in the line-up phase (`OnInitialUpdate`)
  if (state.siege.warState !== WAR_STATE.NotEnterCount) {
    return { state, effects: [box(TID_GAME_GUILDCOMBAT_CANNOT_MAKEUP)] };
  }
  return {
    state,
    effects: [
      { kind: 'needAll', ids: ids([...m.ids, m.defender]) },
      {
        kind: 'open',
        window: 'selection',
        maxJoin: m.maxJoinMember,
        maxWar: m.maxWarPlayer,
        ids: [...m.ids],
        defender: m.defender,
      },
    ],
  };
}

/** `GC_SELECTWARPOS`: the gate choice with its timeout. */
export function reduceGcSelectWarPos(_state: EventState, m: Of<'gcSelectWarPos'>): Reduced {
  return {
    state: _state,
    effects: [{ kind: 'open', window: 'joinSelection', map: m.map, timeoutMs: m.joinTimeS * 1000 }],
  };
}

/** `GC_BESTPLAYER`. */
export function reduceGcBestPlayer(state: EventState, m: Of<'gcBestPlayer'>): Reduced {
  return withSiege(state, { bestPlayer: m.idPlayer });
}

/** `GC_ISREQUEST`. */
export function reduceGcIsRequest(state: EventState, m: Of<'gcIsRequest'>): Reduced {
  return withSiege(state, { requested: m.request });
}

/** `GC_USERSTATE`: the state of a mover; the entity (when it is in view) carries it too. */
export function reduceGcUserState(state: EventState, m: Of<'gcUserState'>): Reduced {
  const userState = new Map(state.siege.userState);
  userState.set(m.moverObjId, m.state);
  return withSiege(state, { userState }, [
    { kind: 'entityState', objId: m.moverObjId, state: m.state },
  ]);
}

/** `GC_WARPLAYERLIST`: the admitted guilds with their players (the HUD roster); every id is queried. */
export function reduceGcWarPlayerList(state: EventState, m: Of<'gcWarPlayerList'>): Reduced {
  const roster = m.guilds.map((g) => ({
    defender: g.defender,
    players: g.players.map((p) => ({ id: p.idPlayer, fighting: p.fighting })),
  }));
  return withSiege(state, { roster }, [
    {
      kind: 'needAll',
      ids: ids(roster.flatMap((g) => [g.defender, ...g.players.map((p) => p.id)])),
    },
  ]);
}

/** `GC_GUILDSTATUS`: the lives of the own guild's players; `max_war_player` is the legacy `nJoinCount`. */
export function reduceGcGuildStatus(state: EventState, m: Of<'gcGuildStatus'>): Reduced {
  const ownLives = m.players.map((p) => ({ id: p.idPlayer, lives: p.lives }));
  return withSiege(state, { ownLives, joinIndex: m.maxWarPlayer }, [
    { kind: 'needAll', ids: ids(ownLives.map((p) => p.id)) },
  ]);
}

/** `GC_GUILDPRECEDENCE`: the guild board; an entry with `send = false` keeps `recv = false`. */
export function reduceGcGuildPrecedence(state: EventState, m: Of<'gcGuildPrecedence'>): Reduced {
  const guildRank = m.entries.map((e) => ({
    name: e.send ? e.guildName : '',
    points: e.send ? e.point : 0,
    recv: e.send,
  }));
  return withSiege(state, { guildRank });
}

/** `GC_PLAYERPRECEDENCE`: the person board (one list per guild); every id is queried. */
export function reduceGcPlayerPrecedence(state: EventState, m: Of<'gcPlayerPrecedence'>): Reduced {
  const playerRank = m.guilds.map((g) =>
    g.players.map((p) => ({ id: p.idPlayer, points: p.point })),
  );
  return withSiege(state, { playerRank }, [
    { kind: 'needAll', ids: ids(playerRank.flatMap((g) => g.map((p) => p.id))) },
  ]);
}

/** `GC_GCSTATE`: the state is stored; a few states say something. */
export function reduceGcGcState(state: EventState, m: Of<'gcGcState'>, env: SiegeEnv): Reduced {
  const effects: SiegeEffect[] = [];
  const s = state.siege;
  let requested = s.requested;
  let roster = s.roster;
  let timer = false;
  if (m.warState === WAR_STATE.NotEnterCount && requested && env.isMaster) {
    effects.push(chat(TID_GAME_GUILDCOMBAT_NOW_PLAYER_MAKEUP));
  }
  // the rest happens in the siege world only
  if (env.worldId === WORLD_GUILDWAR) {
    if (m.state === SIEGE_STATE.War && m.warState === WAR_STATE.Maintenance) {
      effects.push(chat(TID_GAME_GUILDCOMBAT_MAINTENANCE2));
    }
    // the music `BGM_BA_CRISIS` is the audio binding's (T36), not this module's
    if (m.state === SIEGE_STATE.War && m.warState === WAR_STATE.WarWar) {
      effects.push({ kind: 'caption', tid: TID_GAME_GUILDCOMBAT_WAR });
    }
    if (m.state === SIEGE_STATE.CompleteClose && m.warState === WAR_STATE.WarClose) {
      effects.push({ kind: 'caption', tid: TID_GAME_GUILDCOMBAT_WAR_END });
    }
    if (m.state === SIEGE_STATE.CompleteClose && m.warState === WAR_STATE.WarCloseWait) {
      requested = false;
    }
    if (m.state === SIEGE_STATE.Close && m.warState === WAR_STATE.WarTeleport) {
      timer = true;
      roster = [];
    }
  }
  if (timer) effects.push({ kind: 'clearHudTimer' });
  return withSiege(state, { state: m.state, warState: m.warState, requested, roster }, effects);
}

/** `GC_NEXTTIMESTATE`: the HUD timer line, per the state that follows. */
export function reduceGcNextTimeState(
  state: EventState,
  m: Of<'gcNextTimeState'>,
  env: SiegeEnv,
): Reduced {
  const inSiegeWorld = env.worldId === WORLD_GUILDWAR;
  let tid = 0;
  switch (m.state) {
    case WAR_STATE.WarWar:
      if (inSiegeWorld) tid = TID_GAME_GUILDCOMBAT_MAINTENANCE;
      break;
    case WAR_STATE.WarClose:
      if (inSiegeWorld) tid = TID_GAME_GUILDCOMBAT_WAR_END;
      break;
    case WAR_STATE.WarTeleport:
      if (inSiegeWorld) tid = TID_GAME_GUILDCOMBAT_EXIT_MSG;
      break;
    case WAR_STATE.Enter:
      if (state.siege.requested && env.isMasterOrKingpin) {
        tid = TID_GAME_GUILDCOMBAT_PLAYER_MAKEUP_FINISH;
      }
      break;
    default:
      break;
  }
  if (tid === 0) return same(state);
  return { state, effects: [{ kind: 'hudTimer', tid, ms: m.timeMs }] };
}

/** `GC_ENTERTIME`: the time left to enter, only for a guild with an application. */
export function reduceGcEnterTime(state: EventState, m: Of<'gcEnterTime'>): Reduced {
  if (!state.siege.requested) return same(state);
  return {
    state,
    effects: [
      { kind: 'hudTimer', tid: TID_GAME_GUILDCOMBAT_JOIN_LIMIT_TIME, ms: m.timeMs },
      chat(TID_GAME_GUILDCOMBAT_JOIN_NOW),
    ],
  };
}

/** `GC_DIAGMESSAGE`: an information box. */
export function reduceGcDiagMessage(state: EventState, m: Of<'gcDiagMessage'>): Reduced {
  return { state, effects: [{ kind: 'open', window: 'infoBox2', text: m.text }] };
}

/** `GC_TELE`: an information box with a leave button. */
export function reduceGcTele(state: EventState, m: Of<'gcTele'>): Reduced {
  return { state, effects: [{ kind: 'open', window: 'infoBox', text: m.text }] };
}

/** The wire entry of `GC_LOG` / `GC_LOG_REALTIME` as a log entry. */
export function logEntryOf(e: {
  guildAttack: number;
  guildDefence: number;
  playerAttack: number;
  playerDefence: number;
  point: number;
  killOtherGuild: boolean;
  master: boolean;
  defender: boolean;
  lastLife: boolean;
}): LogEntry {
  return {
    guildAttack: e.guildAttack,
    guildDefence: e.guildDefence,
    playerAttack: e.playerAttack,
    playerDefence: e.playerDefence,
    point: e.point,
    killOtherGuild: e.killOtherGuild,
    master: e.master,
    defender: e.defender,
    lastLife: e.lastLife,
  };
}

/** `GC_LOG`: the result window, ALWAYS (an empty log still opens it). */
export function reduceGcLog(state: EventState, m: Of<'gcLog'>): Reduced {
  const entries = m.entries.map(logEntryOf);
  return {
    state,
    effects: [
      {
        kind: 'needAll',
        ids: ids(entries.flatMap((e) => [e.playerAttack, e.playerDefence])),
      },
      { kind: 'open', window: 'result', entries },
    ],
  };
}

/** `GC_LOG_REALTIME`: "[guild][attacker] has killed [guild][victim] and has gained N points." as a white general chat line. */
export function reduceGcLogRealtime(
  state: EventState,
  m: Of<'gcLogRealtime'>,
  env: SiegeEnv,
): Reduced {
  const text = realtimeLine(
    env.guildName(m.guildAttack) ?? NOT_GUILD,
    m.attackerName,
    env.guildName(m.guildDefence) ?? NOT_GUILD,
    m.defenderName,
    m.point,
  );
  return { state, effects: [{ kind: 'chatText', text }] };
}

/** The overflow texts shared by the two claims (result 3 of the guild claim, result 2 of the player claim). */
function overflow(penya: number, purse: number): SiegeEffect[] {
  if (penya > I32_MAX) return [box(TID_GAME_GUILDCOMBAT_JACKPOT_LIMIT2)];
  if (purse + penya > I32_MAX) return [box(TID_GAME_GUILDCOMBAT_JACKPOT_LIMIT1)];
  return [];
}

/** `GC_GETPENYAGUILD`: 0 the prize, 1 the refund, 2 nothing to claim, 3 the amount does not fit the purse. */
export function reduceGcGetPenyaGuild(
  state: EventState,
  m: Of<'gcGetPenyaGuild'>,
  env: SiegeEnv,
): Reduced {
  const penya = Number(m.penya);
  switch (m.result) {
    case 0:
      return { state, effects: [box(TID_GAME_GUILDCOMBAT_JACKPOT, penya)] };
    case 1:
      return { state, effects: [box(TID_GAME_GUILDCOMBAT_JACKPOT_CANCEL, penya)] };
    case 2:
      return { state, effects: [box(TID_GAME_GUILDCOMBAT_WINNER_NOT_GUILD)] };
    case 3:
      return { state, effects: overflow(penya, env.purse) };
    default:
      return same(state);
  }
}

/** `GC_GETPENYAPLAYER`: 0 the MVP prize, 1 not the best player, 2 the amount does not fit the purse. */
export function reduceGcGetPenyaPlayer(
  state: EventState,
  m: Of<'gcGetPenyaPlayer'>,
  env: SiegeEnv,
): Reduced {
  const penya = Number(m.penya);
  switch (m.result) {
    case 0:
      return { state, effects: [box(TID_GAME_GUILDCOMBAT_MVP_PENYA, penya)] };
    case 1:
      return { state, effects: [box(TID_GAME_GUILDCOMBAT_NOT_MVP)] };
    case 2:
      return { state, effects: overflow(penya, env.purse) };
    default:
      return same(state);
  }
}

/** `GC_PLAYERPOINT`: the person ranking window; the names are queried. */
export function reduceGcPlayerPoint(state: EventState, m: Of<'gcPlayerPoint'>): Reduced {
  const entries = m.entries.map((e) => ({ id: e.idPlayer, job: e.job, points: e.point }));
  return {
    state,
    effects: [
      { kind: 'needAll', ids: ids(entries.map((e) => e.id)) },
      { kind: 'open', window: 'rankPerson', entries },
    ],
  };
}

// ---- the table -------------------------------------------------------------------------------------------------------------------------

type Reducer<C extends Case> = (state: EventState, m: Of<C>, env: SiegeEnv) => Reduced;

/** Every case of the oneof and its reducer (`every_guild_combat_sub_is_handled` iterates the descriptor against this table). */
export const SIEGE_REDUCERS: { readonly [C in Case]: Reducer<C> } = {
  gcWinGuild: reduceGcWinGuild,
  gcInWindow: reduceGcInWindow,
  gcInComplete: reduceGcInComplete,
  gcRequestStatus: reduceGcRequestStatus,
  gcSelectPlayer: reduceGcSelectPlayer,
  gcSelectWarPos: reduceGcSelectWarPos,
  gcBestPlayer: reduceGcBestPlayer,
  gcIsRequest: reduceGcIsRequest,
  gcUserState: reduceGcUserState,
  gcWarPlayerList: reduceGcWarPlayerList,
  gcGuildStatus: reduceGcGuildStatus,
  gcGuildPrecedence: reduceGcGuildPrecedence,
  gcPlayerPrecedence: reduceGcPlayerPrecedence,
  gcGcState: reduceGcGcState,
  gcNextTimeState: reduceGcNextTimeState,
  gcEnterTime: reduceGcEnterTime,
  gcDiagMessage: reduceGcDiagMessage,
  gcTele: reduceGcTele,
  gcLog: reduceGcLog,
  gcLogRealtime: reduceGcLogRealtime,
  gcGetPenyaGuild: reduceGcGetPenyaGuild,
  gcGetPenyaPlayer: reduceGcGetPenyaPlayer,
  gcPlayerPoint: reduceGcPlayerPoint,
};

/** Applies one `GuildCombat` message; an empty oneof changes nothing. */
export function reduceGuildCombat(
  state: EventState,
  sub: GuildCombat['sub'],
  env: SiegeEnv,
): Reduced {
  if (sub.case === undefined) return same(state);
  const reducer = SIEGE_REDUCERS[sub.case] as (s: EventState, m: Sub, e: SiegeEnv) => Reduced;
  return reducer(state, sub.value, env);
}
