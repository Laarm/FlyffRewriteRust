// The reducers of the rainbow race (Plan/45 §28.7, ref/09b §3.16 "Rainbow race"; `DPClient.cpp:18143-18489`, `WndWorld.cpp:9169-9201`; WP-CFEAT-44): PURE functions from the slice and a message to the new slice and a list of
// effects (data that the module carries out). Every message reduces whether or not a window is open (45 §28.0).
//
//   reduceNowState      `RainbowRaceNowState` -> the slice (`CRainbowRace::Serialize`)
//   reduceMiniGame      `RainbowRaceMiniGameState` / `...ExtState` -> effects: open / deliver / close / freezeProgress / endBox / box; the plain message has `data = []`
//   reduceLifecycle     the race button and the all-done box (the per-frame block of `CWndWorld::Process`)
//   endBoxClosed        `CWndRainbowRaceMiniGameEnd::OnDestroy`: `m_bCheckEnd = TRUE`

import {
  TID_GAME_RAINBOWRACE_MINIGAMEFINISH,
  TID_GAME_REMOVE_ARITHMETIC_FAIL,
} from '../../../gen/defs/text';
import { ALL_COMPLETED, MP, RMG } from '../../../ui/windows/rainbow/constants';
import { gameKeyOf } from '../../../ui/windows/rainbow/glow';
import { racingOf, type RainbowSlice } from './state';

/** `RainbowRaceNowState{ race{ now_game, completed_mask, remain_ms } }`. */
export interface NowStateMsg {
  readonly nowGame: number;
  readonly completed: number;
  readonly remainMs: number;
}

/** A mini-game packet (`__MINIGAME_PACKET`; the ext one adds `data`). */
export interface MiniGameMsg {
  readonly nowGame: number;
  readonly state: number;
  readonly p1: number;
  readonly p2: number;
  readonly data: readonly string[];
}

/** What the OPENWND of a game hands to its window (the first values, applied while the window is made). */
export interface GameInit {
  /** Dice and stopwatch: `p1`. */
  readonly target?: number;
  /** Arithmetic: the question (the LAST string of the packet) and the correct answers so far. */
  readonly question?: string;
  readonly count?: number;
  /** Typing: the questions in order. */
  readonly questions?: readonly string[];
  /** Cards: the 18-byte deck. */
  readonly deck?: string;
}

/** A call on an open game window. */
export type GameCall =
  | { readonly call: 'result'; readonly state: number }
  | {
      readonly call: 'diceResult';
      readonly state: number;
      readonly p1: number;
      readonly p2: number;
    }
  | { readonly call: 'setQuestion'; readonly question: string; readonly count: number }
  | { readonly call: 'setTarget'; readonly ms: number }
  | { readonly call: 'typingReset' };

export type RainbowEffect =
  | { readonly kind: 'open'; readonly game: number; readonly init: GameInit }
  | { readonly kind: 'deliver'; readonly game: number; readonly call: GameCall }
  | { readonly kind: 'close'; readonly game: number }
  | { readonly kind: 'freezeProgress' }
  | { readonly kind: 'endBox'; readonly game: number }
  | { readonly kind: 'box'; readonly tid: number };

/** `CRainbowRace::Serialize`: `remain > 0 => remain + now`, else 0 (no `endMs`). */
export function reduceNowState(s: RainbowSlice, m: NowStateMsg, now: number): RainbowSlice {
  const base = {
    nowGame: m.nowGame,
    completed: m.completed,
    checkEnd: s.checkEnd,
    finishShown: s.finishShown,
  };
  return m.remainMs > 0 ? { ...base, endMs: now + m.remainMs } : base;
}

const freeze: RainbowEffect = { kind: 'freezeProgress' };
const open = (game: number, init: GameInit = {}): RainbowEffect => ({ kind: 'open', game, init });
const deliver = (game: number, call: GameCall): RainbowEffect => ({ kind: 'deliver', game, call });
const endGame = (game: number): RainbowEffect[] => [
  freeze,
  { kind: 'endBox', game },
  { kind: 'close', game },
];

/** The LAST string of the packet (`for ( i ) strQuestion = data[ i ]`), '' when there is none. */
const lastOf = (data: readonly string[]): string => data[data.length - 1] ?? '';

/**
 * `OnRainbowRaceMiniGameState`: ignored unless the packet belongs to the stored game (`m_wNowGame & packet.wNowGame`); then it switches on the lowest set bit of the STORED game
 * (not of the packet) and acts per game and state as the table of the original. FIX: a card OPENWND without a string is ignored (the original read an empty vector).
 */
export function reduceMiniGame(s: RainbowSlice, m: MiniGameMsg): RainbowEffect[] {
  if ((s.nowGame & m.nowGame) === 0) return [];
  const game = gameKeyOf(s.nowGame);
  if (game < 0) return [];
  const { state, p1 } = m;
  switch (game) {
    case RMG.KAWIBAWIBO:
      if (state === MP.OPENWND) return [open(game)];
      return [...(state === MP.FINISH ? [freeze] : []), deliver(game, { call: 'result', state })];
    case RMG.DICE:
      if (state === MP.OPENWND) return [open(game, { target: p1 })];
      return [
        ...(state === MP.FINISH ? [freeze] : []),
        deliver(game, { call: 'diceResult', state, p1, p2: m.p2 }),
      ];
    case RMG.ARITHMETIC: {
      const question = lastOf(m.data);
      if (state === MP.OPENWND) return [open(game, { question, count: p1 })];
      if (state === MP.TRUE || state === MP.FAIL) {
        return [deliver(game, { call: 'setQuestion', question, count: p1 })];
      }
      if (state === MP.FALSE) {
        return [
          { kind: 'box', tid: TID_GAME_REMOVE_ARITHMETIC_FAIL },
          deliver(game, { call: 'setQuestion', question, count: p1 }),
        ];
      }
      return state === MP.FINISH ? endGame(game) : [];
    }
    case RMG.STOPWATCH:
      if (state === MP.OPENWND) return [open(game, { target: p1 })];
      if (state === MP.FAIL) {
        return [
          { kind: 'box', tid: TID_GAME_REMOVE_ARITHMETIC_FAIL },
          deliver(game, { call: 'setTarget', ms: p1 }),
        ];
      }
      return state === MP.FINISH ? endGame(game) : [];
    case RMG.TYPING:
      if (state === MP.OPENWND) return [open(game, { questions: m.data.slice(0, 3) })];
      if (state === MP.FAIL) {
        return [
          { kind: 'box', tid: TID_GAME_REMOVE_ARITHMETIC_FAIL },
          deliver(game, { call: 'typingReset' }),
        ];
      }
      return state === MP.FINISH ? endGame(game) : [];
    case RMG.CARD: {
      if (state === MP.OPENWND) {
        const deck = m.data[0];
        return deck === undefined ? [] : [open(game, { deck })];
      }
      if (state === MP.TRUE || state === MP.FALSE || state === MP.FAIL) {
        return [deliver(game, { call: 'result', state })];
      }
      return state === MP.FINISH ? endGame(game) : [];
    }
    case RMG.LADDER:
      if (state === MP.OPENWND) return [open(game)];
      if (state === MP.FAIL) return [deliver(game, { call: 'result', state })];
      if (state === MP.FINISH) return [freeze, deliver(game, { call: 'result', state })];
      return [];
    default:
      return [];
  }
}

/** The effects of the race button lifecycle. */
export type LifecycleEffect =
  | { readonly kind: 'openButton' }
  | { readonly kind: 'closeButton' }
  | { readonly kind: 'closeProgress' }
  | { readonly kind: 'box'; readonly tid: number };

export interface LifecycleResult {
  readonly slice: RainbowSlice;
  readonly effects: LifecycleEffect[];
}

/**
 * The per-frame block of `CWndWorld::Process` (`WndWorld.cpp:9169-9201`): the race button exists exactly while `racing && flagOn` (FIX/gate: the flag is the client gate of event 25). Making it resets
 * `checkEnd` and `finishShown`; while it exists `completed != 0x7F` clears `checkEnd`, and `completed == 0x7F && checkEnd && !finishShown` shows the all-done box once. Without the race the
 * button and the progress window go.
 */
export function reduceLifecycle(
  s: RainbowSlice,
  env: { readonly flagOn: boolean; readonly buttonOpen: boolean },
): LifecycleResult {
  const visible = racingOf(s) && env.flagOn;
  if (!visible) {
    return {
      slice: s,
      effects: env.buttonOpen ? [{ kind: 'closeButton' }, { kind: 'closeProgress' }] : [],
    };
  }
  if (!env.buttonOpen) {
    return {
      slice: s.checkEnd || s.finishShown ? { ...s, checkEnd: false, finishShown: false } : s,
      effects: [{ kind: 'openButton' }],
    };
  }
  if (s.completed === ALL_COMPLETED) {
    if (s.checkEnd && !s.finishShown) {
      return {
        slice: { ...s, finishShown: true },
        effects: [{ kind: 'box', tid: TID_GAME_RAINBOWRACE_MINIGAMEFINISH }],
      };
    }
    return { slice: s, effects: [] };
  }
  return s.checkEnd ? { slice: { ...s, checkEnd: false }, effects: [] } : { slice: s, effects: [] };
}

/** `CWndRainbowRaceMiniGameEnd::OnDestroy`: `m_bCheckEnd = TRUE` (only while the race runs: a box closed by the logout sweep must not mark a slice that was just reset). */
export function endBoxClosed(s: RainbowSlice): RainbowSlice {
  return racingOf(s) && !s.checkEnd ? { ...s, checkEnd: true } : s;
}

/** The ranking ids padded to five (`adwPlayerId[ 5 ] = { 0 }`; more than five are not read). */
export function rankingIds(ids: readonly number[]): number[] {
  return Array.from({ length: 5 }, (_, i) => ids[i] ?? 0);
}
