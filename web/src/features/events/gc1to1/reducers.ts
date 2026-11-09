// The reducers of the 1:1 guild combat messages (`src:Neuz/DPClient.cpp:16846-17050`, `_Common/GuildCombat1to1.cpp:703-767`, Plan/45 §28.5; WP-CFEAT-45): PURE, `( slice, message, env ) -> { slice, effects }`;
// the effects are data that the module carries out. Every message is applied whether or not a window is open (PL-124); the module drops all of them while event 22 is off.
//
//   Gc1to1NowState{ state, time_ms, members, enemy_guild, prog_count }
//     the state is stored for EVERY value (`m_nState = nState`: the invite rule of the guild window also refuses in the arena states), then by state:
//       1 Open        only the guild master: the HUD label READY_TIME for `time_ms`; the wait flag drops (a non-master changes nothing else: KEEP, the original set no label)
//       2 Entrance    results and wins reset, the line-up = `members` (every row Ready), the opposing guild, the label ENTRANCE_TIME, the info box TELEPORT_MSG (YES = Gc1to1TeleportToNpc)
//       101 Wait      the label WAIT_TIME( prog_count + 1 ) counted in seconds (`waitSec`), the round = prog_count
//       102 War       the label WAR_TIME            104 CloseWait   the label WAR_CLOSEWAIT
//       0, 100, 3     the label and the timer go (unconditional: KEEP), the wait flag drops
//       103, others   the state only
//   Gc1to1WarResult{ state, id_player, win_count0, win_count1 }
//     `results[ id_player ] = state` only for an id of the line-up (the original updated existing rows only); every state but Ready 200 and War 201 sets the round wins = ( OURS, THEIRS ): the
//     server sends per receiver, `win_count0` is the recipient guild's (`GuildCombat1to1.cpp:195-199`). An `id_player` beyond the line-up (0) changes no row but still sets the wins.
//   Gc1to1MemberLineUpOpenWndNtf{ min_level, min_players, max_players, ids }   the limits are stored, the line-up editor opens with the stored ids

import {
  TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE_TIME,
  TID_GAME_GUILDCOMBAT_1TO1_READY_TIME,
  TID_GAME_GUILDCOMBAT_1TO1_TELEPORT_MSG,
  TID_GAME_GUILDCOMBAT_1TO1_WAIT_TIME,
  TID_GAME_GUILDCOMBAT_1TO1_WAR_CLOSEWAIT,
  TID_GAME_GUILDCOMBAT_1TO1_WAR_TIME,
} from '../../../gen/defs/text';
import { GC1TO1_ARENA, GC1TO1_PLAYER, GC1TO1_STATE, type Gc1to1Slice } from './slice';

/** The data of `Gc1to1NowState` (the proto message's fields, as numbers). */
export interface NowStateMsg {
  readonly state: number;
  readonly timeMs: number;
  readonly members: readonly number[];
  readonly enemyGuild: number;
  readonly progCount: number;
}

/** The data of `Gc1to1WarResult`. */
export interface WarResultMsg {
  readonly state: number;
  readonly idPlayer: number;
  readonly winCount0: number;
  readonly winCount1: number;
}

/** The data of `Gc1to1MemberLineUpOpenWndNtf`. */
export interface LineUpOpenMsg {
  readonly minLevel: number;
  readonly minPlayers: number;
  readonly maxPlayers: number;
  readonly ids: readonly number[];
}

export interface Gc1to1Env {
  /** The own character is the master of the own guild (`m_nMemberLv == GUD_MASTER`). */
  readonly selfIsMaster: boolean;
}

export type Gc1to1Effect =
  /** The ONE HUD timer line: the label of `tid` (with `args`) for `ms`; `waitSec` shows the seconds with `TID_GAME_GUILDCOMBAT_1TO1_WAITSEC`. */
  | {
      readonly kind: 'hudTimer';
      readonly tid: number;
      readonly args: readonly number[];
      readonly ms: number;
      readonly waitSec: boolean;
    }
  /** The label and the timer go (`m_dwGuildCombatTime = 0xffffffff`), whoever wrote them. */
  | { readonly kind: 'clearHudTimer' }
  /** `CGuildCombatInfoMessageBox( 1 )`: YES sends `Gc1to1TeleportToNpc`. */
  | { readonly kind: 'infoBox'; readonly tid: number }
  /** `SendQueryPlayerData`: the names of the line-up for the HUD. */
  | { readonly kind: 'needAll'; readonly ids: readonly number[] }
  /** The line-up editor with the stored ids. */
  | { readonly kind: 'openLineUp'; readonly ids: readonly number[] };

export interface Reduced {
  readonly slice: Gc1to1Slice;
  readonly effects: readonly Gc1to1Effect[];
}

const timer = (
  tid: number,
  ms: number,
  args: readonly number[] = [],
  waitSec = false,
): Gc1to1Effect => ({
  kind: 'hudTimer',
  tid,
  args,
  ms,
  waitSec,
});

/** `OnGC1to1NowState`. */
export function reduceNowState(slice: Gc1to1Slice, m: NowStateMsg, env: Gc1to1Env): Reduced {
  const next = { ...slice, state: m.state };
  switch (m.state) {
    case GC1TO1_STATE.Open:
      if (!env.selfIsMaster) return { slice: next, effects: [] };
      return {
        slice: { ...next, waiting: false },
        effects: [timer(TID_GAME_GUILDCOMBAT_1TO1_READY_TIME, m.timeMs)],
      };
    case GC1TO1_STATE.Entrance: {
      const lineup = [...m.members];
      return {
        slice: {
          ...next,
          lineup,
          opponentGuild: m.enemyGuild,
          results: new Map(lineup.map((id) => [id, GC1TO1_PLAYER.Ready])),
          wins: [0, 0],
          waiting: false,
        },
        effects: [
          timer(TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE_TIME, m.timeMs),
          { kind: 'needAll', ids: lineup },
          { kind: 'infoBox', tid: TID_GAME_GUILDCOMBAT_1TO1_TELEPORT_MSG },
        ],
      };
    }
    case GC1TO1_ARENA.Wait:
      return {
        slice: { ...next, round: m.progCount, waiting: true },
        effects: [timer(TID_GAME_GUILDCOMBAT_1TO1_WAIT_TIME, m.timeMs, [m.progCount + 1], true)],
      };
    case GC1TO1_ARENA.War:
      return {
        slice: { ...next, waiting: false },
        effects: [timer(TID_GAME_GUILDCOMBAT_1TO1_WAR_TIME, m.timeMs)],
      };
    case GC1TO1_ARENA.CloseWait:
      return {
        slice: { ...next, waiting: false },
        effects: [timer(TID_GAME_GUILDCOMBAT_1TO1_WAR_CLOSEWAIT, m.timeMs)],
      };
    case GC1TO1_STATE.Close:
    case GC1TO1_ARENA.WarClose:
    case GC1TO1_STATE.War:
      return { slice: { ...next, waiting: false }, effects: [{ kind: 'clearHudTimer' }] };
    default:
      // 103 Finish and anything else: the state only
      return { slice: next, effects: [] };
  }
}

/** `SetGuildCombat1to1State`. */
export function reduceWarResult(slice: Gc1to1Slice, m: WarResultMsg): Gc1to1Slice {
  let results: ReadonlyMap<number, number> = slice.results;
  if (slice.lineup.includes(m.idPlayer) && results.get(m.idPlayer) !== m.state) {
    const copy = new Map(results);
    copy.set(m.idPlayer, m.state);
    results = copy;
  }
  const final = m.state !== GC1TO1_PLAYER.Ready && m.state !== GC1TO1_PLAYER.War;
  const wins: readonly [number, number] = final ? [m.winCount0, m.winCount1] : slice.wins;
  if (results === slice.results && wins === slice.wins) return slice;
  return { ...slice, results, wins };
}

/** `OnGC1to1MemberLineUpOpenWnd`: the limits and the stored line-up. */
export function reduceLineUp(slice: Gc1to1Slice, m: LineUpOpenMsg): Reduced {
  return {
    slice: { ...slice, limits: { minLevel: m.minLevel, min: m.minPlayers, max: m.maxPlayers } },
    effects: [{ kind: 'openLineUp', ids: [...m.ids] }],
  };
}
