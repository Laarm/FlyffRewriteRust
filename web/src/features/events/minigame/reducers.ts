// The reducers of the `MiniGame` multiplexer (`src:Neuz/DPClient.cpp:11433-11650`, Plan/45 §28.0; WP-CFEAT-45): one handler per sub. Every sub is applied to the OPEN window's machine and dropped when the
// window is absent (a WIN without the Kawibawibo window crashed the C++; the others dereferenced nothing), except the two results that are shown whether or not a window exists: the GETITEM box of
// Kawibawibo and the chat line of the reassemble result. A handler mutates the machines it is given (they ARE the state of an open game) and answers the effects the module carries out.
//
//   KawibawiboResult   WIN -> the prizes and the highest streak into the machine; GETITEM -> the box `TID_GAME_KAWIBAWIBO_WIN( streak, item, count )`, the streak is paid out; the others -> the machine
//   ReassembleResult   PL-132: the chat line "Success <item> <count>" (new English text, `TID_ADMIN_ANNOUNCE` colour, game-chat style); the open window then shows `TID_GAME_PUZZLE_GIVEPRESENT`,
//                      plays the win cue and closes; item 0 (the server found no prize) -> no line, no box, the window closes
//   ReassembleOpenWnd  the nine card ids into the puzzle machine
//   AlphabetOpenWnd    the question id, the hint letter and the question into the word machine
//   AlphabetResult     999 -> `TID_GAME_FINDWORD_FAIL`, 998 -> `TID_GAME_LACKMONEY`, else `TID_GAME_PUZZLE_GIVEPRESENT`; then the window closes
//   FiveSystemOpenWnd  min / max / multiple into the dice machine
//   FiveSystemResult   996 -> the stake of a face (`bet_num`, `penya`); 997 / 998 / 999 change nothing; 0..5 -> the roll and the payout

import {
  TID_ADMIN_ANNOUNCE,
  TID_GAME_FINDWORD_FAIL,
  TID_GAME_KAWIBAWIBO_WIN,
  TID_GAME_LACKMONEY,
  TID_GAME_PUZZLE_GIVEPRESENT,
} from '../../../gen/defs/text';
import type { MiniGame } from '../../../gen/proto/flyff/v1/event_pb';
import { CHATSTY } from '../../../ui/chat/styles';
import { ALPHABET_RESULT, type FindWordMachine } from './findWord';
import { FIVE_RESULT, type DiceMachine } from './dice';
import { KAWI_RESULT, type KawibawiboMachine } from './kawibawibo';
import type { PuzzleMachine } from './puzzle';

/** The open games (the machines of the windows that exist now). */
export interface MiniGameMachines {
  readonly kawi: KawibawiboMachine | null;
  readonly word: FindWordMachine | null;
  readonly puzzle: PuzzleMachine | null;
  readonly dice: DiceMachine | null;
}

export type MiniGameEffect =
  | { readonly kind: 'box'; readonly tid: number; readonly args?: readonly (string | number)[] }
  | {
      readonly kind: 'chat';
      readonly text: string;
      readonly colorTid: number;
      readonly style: number;
    }
  | { readonly kind: 'close'; readonly window: 'word' | 'puzzle' }
  | { readonly kind: 'cue'; readonly cue: 'win' };

export interface MiniGameEnv {
  /** `ItemProp::szName`; `''` for an unknown item (PL-105: the C++ dereferenced NULL). */
  itemName(itemId: number): string;
}

type Sub = Exclude<MiniGame['sub'], { case: undefined }>;
type Handler<K extends Sub['case']> = (
  m: MiniGameMachines,
  value: Extract<Sub, { case: K }>['value'],
  env: MiniGameEnv,
) => readonly MiniGameEffect[];

/** The text of the PL-132 line: new English text replacing the hard-coded Korean `성공 %s %d`. */
export function reassembleLine(name: string, count: number): string {
  return `Success ${name} ${String(count)}`;
}

export const reduceKawibawiboResult: Handler<'kawibawiboResult'> = (m, r, env) => {
  const reply = {
    result: r.result,
    winCount: r.winCount,
    itemId: r.itemId,
    nextItemId: r.nextItemId,
    itemCount: r.itemCount,
    nextItemCount: r.nextItemCount,
    maxWinCount: r.maxWinCount,
  };
  if (r.result === KAWI_RESULT.GETITEM) {
    // shown whether or not the window is open: the streak of the open window, else the one of the message
    const streak = m.kawi ? m.kawi.winning : r.winCount;
    m.kawi?.gotItem();
    return [
      {
        kind: 'box',
        tid: TID_GAME_KAWIBAWIBO_WIN,
        args: [streak, env.itemName(r.itemId), r.itemCount],
      },
    ];
  }
  m.kawi?.receive(reply);
  return [];
};

export const reduceReassembleResult: Handler<'reassembleResult'> = (m, r, env) => {
  const out: MiniGameEffect[] = [];
  const found = r.itemId !== 0;
  if (found) {
    out.push({
      kind: 'chat',
      text: reassembleLine(env.itemName(r.itemId), r.count),
      colorTid: TID_ADMIN_ANNOUNCE,
      style: CHATSTY.GAME_CHAT,
    });
  }
  if (m.puzzle) {
    if (found) {
      out.push({ kind: 'box', tid: TID_GAME_PUZZLE_GIVEPRESENT }, { kind: 'cue', cue: 'win' });
    }
    out.push({ kind: 'close', window: 'puzzle' });
  }
  return out;
};

export const reduceReassembleOpenWnd: Handler<'reassembleOpenWnd'> = (m, r) => {
  m.puzzle?.setCards(r.cardItemIds);
  return [];
};

export const reduceAlphabetOpenWnd: Handler<'alphabetOpenWnd'> = (m, r) => {
  m.word?.setQuestion(r.questionId, r.publicPos, r.publicChar, r.question);
  return [];
};

export const reduceAlphabetResult: Handler<'alphabetResult'> = (m, r) => {
  if (!m.word) return [];
  const tid =
    r.result === ALPHABET_RESULT.FAILED
      ? TID_GAME_FINDWORD_FAIL
      : r.result === ALPHABET_RESULT.NOTENOUGH_MONEY
        ? TID_GAME_LACKMONEY
        : TID_GAME_PUZZLE_GIVEPRESENT;
  return [
    { kind: 'box', tid },
    { kind: 'close', window: 'word' },
  ];
};

export const reduceFiveSystemOpenWnd: Handler<'fiveSystemOpenWnd'> = (m, r) => {
  m.dice?.setLimits(r.minBet, r.maxBet, r.multiple);
  return [];
};

export const reduceFiveSystemResult: Handler<'fiveSystemResult'> = (m, r) => {
  if (!m.dice) return [];
  if (r.result === FIVE_RESULT.BETOK) m.dice.setStake(r.betNum, r.penya);
  else if (r.result >= 0 && r.result < 6) m.dice.receiveRoll(r.result, r.penya);
  // FAILED, OVERMAX and NOTENOUGH change nothing (KEEP)
  return [];
};

/** One handler per oneof case of `MiniGame.sub` (`every_minigame_sub_is_handled` compares the keys with the descriptor). */
export const MINIGAME_REDUCERS: { readonly [K in Sub['case']]: Handler<K> } = {
  kawibawiboResult: reduceKawibawiboResult,
  reassembleResult: reduceReassembleResult,
  reassembleOpenWnd: reduceReassembleOpenWnd,
  alphabetOpenWnd: reduceAlphabetOpenWnd,
  alphabetResult: reduceAlphabetResult,
  fiveSystemOpenWnd: reduceFiveSystemOpenWnd,
  fiveSystemResult: reduceFiveSystemResult,
};

/** Applies one `MiniGame` message to the open machines. */
export function reduceMiniGame(
  m: MiniGameMachines,
  sub: MiniGame['sub'],
  env: MiniGameEnv,
): readonly MiniGameEffect[] {
  switch (sub.case) {
    case 'kawibawiboResult':
      return reduceKawibawiboResult(m, sub.value, env);
    case 'reassembleResult':
      return reduceReassembleResult(m, sub.value, env);
    case 'reassembleOpenWnd':
      return reduceReassembleOpenWnd(m, sub.value, env);
    case 'alphabetOpenWnd':
      return reduceAlphabetOpenWnd(m, sub.value, env);
    case 'alphabetResult':
      return reduceAlphabetResult(m, sub.value, env);
    case 'fiveSystemOpenWnd':
      return reduceFiveSystemOpenWnd(m, sub.value, env);
    case 'fiveSystemResult':
      return reduceFiveSystemResult(m, sub.value, env);
    case undefined:
      return [];
  }
}
