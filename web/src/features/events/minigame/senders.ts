// The sends of the mini-game windows (rule C9: `// legacy-send: <SendFn>`). The ten functions of WP-CFEAT-45 (`src:_Interface/WndMiniGame.cpp`, `Neuz/DPClient.cpp` `SendKawibawibo*` ..
// `SendFiveSystem*`). The session identifies the sender (VR-001). The answers of the server are the `MiniGame` multiplexer (`reducers.ts`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  AlphabetOpenWndSchema,
  AlphabetStartSchema,
  FiveSystemBetSchema,
  FiveSystemDestroyWndSchema,
  FiveSystemOpenWndSchema,
  FiveSystemStartSchema,
  KawibawiboGetItemSchema,
  KawibawiboStartSchema,
  ReassembleOpenWndSchema,
  ReassembleStartSchema,
} from '../../../gen/proto/flyff/v1/event_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders over one client context. */
export interface MiniGameSenders {
  /** `SendKawibawiboStart()`: a round (needs the coupon unless the last round was a draw or a win). */
  kawibawiboStart(): void;
  /** `SendKawibawiboGetItem()`: the prize of the streak. */
  kawibawiboGetItem(): void;
  /** `SendAlphabetOpenWnd()`: the find-word window opened. */
  alphabetOpenWnd(): void;
  /** `SendAlphabetStart( objIds, 5, questionId )`. */
  alphabetStart(questionId: number, objIds: readonly number[]): void;
  /** `SendReassembleOpenWnd()`. */
  reassembleOpenWnd(): void;
  /** `SendReassembleStart( objIds, 9 )`. */
  reassembleStart(objIds: readonly number[]): void;
  /** `SendFiveSystemOpenWnd()`. */
  fiveSystemOpenWnd(): void;
  /** `SendFiveSystemDestroyWnd()`: the dice window closed (the server refunds the stakes). */
  fiveSystemDestroyWnd(): void;
  /** `SendFiveSystemBet( face, penya )`: 0 withdraws the stake. */
  fiveSystemBet(betNum: number, betPenya: number): void;
  /** `SendFiveSystemStart()`. */
  fiveSystemStart(): void;
}

export function miniGameSenders(send: Send): MiniGameSenders {
  return {
    kawibawiboStart() {
      // legacy-send: SendKawibawiboStart
      send({ case: 'kawibawiboStart', value: create(KawibawiboStartSchema, {}) });
    },
    kawibawiboGetItem() {
      // legacy-send: SendKawibawiboGetItem
      send({ case: 'kawibawiboGetItem', value: create(KawibawiboGetItemSchema, {}) });
    },
    alphabetOpenWnd() {
      // legacy-send: SendAlphabetOpenWnd
      send({ case: 'alphabetOpenWnd', value: create(AlphabetOpenWndSchema, {}) });
    },
    alphabetStart(questionId, objIds) {
      // legacy-send: SendAlphabetStart
      send({
        case: 'alphabetStart',
        value: create(AlphabetStartSchema, { questionId, itemObjIds: [...objIds] }),
      });
    },
    reassembleOpenWnd() {
      // legacy-send: SendReassembleOpenWnd
      send({ case: 'reassembleOpenWnd', value: create(ReassembleOpenWndSchema, {}) });
    },
    reassembleStart(objIds) {
      // legacy-send: SendReassembleStart
      send({
        case: 'reassembleStart',
        value: create(ReassembleStartSchema, { itemObjIds: [...objIds] }),
      });
    },
    fiveSystemOpenWnd() {
      // legacy-send: SendFiveSystemOpenWnd
      send({ case: 'fiveSystemOpenWnd', value: create(FiveSystemOpenWndSchema, {}) });
    },
    fiveSystemDestroyWnd() {
      // legacy-send: SendFiveSystemDestroyWnd
      send({ case: 'fiveSystemDestroyWnd', value: create(FiveSystemDestroyWndSchema, {}) });
    },
    fiveSystemBet(betNum, betPenya) {
      // legacy-send: SendFiveSystemBet
      send({
        case: 'fiveSystemBet',
        value: create(FiveSystemBetSchema, { betNum, betPenya }),
      });
    },
    fiveSystemStart() {
      // legacy-send: SendFiveSystemStart
      send({ case: 'fiveSystemStart', value: create(FiveSystemStartSchema, {}) });
    },
  };
}
