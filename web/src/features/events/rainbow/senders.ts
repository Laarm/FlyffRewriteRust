// The sends of the rainbow race (rule C9: `// legacy-send: <SendFn>`): the six functions of `DPClient.cpp:18103-18141`. The session identifies the sender (VR-001), so the packets carry no ids. The play packets use the STORED
// game (`CRainbowRace::SendMinigamePacket( RMG_MAX, MP_NONE, p1, p2 )`); the menus 113-119 name the game they open (`SendMinigamePacket( game, MP_OPENWND )`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  MiniGamePacketSchema,
  RainbowRaceApplicationOpenWndReqSchema,
  RainbowRaceApplicationSchema,
  RainbowRaceMiniGamePacketSchema,
  RainbowRacePrevRankingOpenWndReqSchema,
  RainbowRaceReqFinishSchema,
} from '../../../gen/proto/flyff/v1/event_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

export interface RainbowSenders {
  /** `SendRainbowRaceApplicationOpenWnd()`: menu 107. */
  applicationOpenWnd(): void;
  /** `SendRainbowRaceApplication()`: OK of the application window. */
  application(): void;
  /** `SendRainbowRacePrevRankingOpenWnd()`: menu 110. */
  prevRankingOpenWnd(): void;
  /** `SendRainbowRaceReqFinish()`: menu 112. */
  reqFinish(): void;
  /** `SendRainbowRaceMiniGameState( { nowGame, state, p1, p2 } )`: the plain packet. */
  miniGameState(nowGame: number, state: number, p1: number, p2: number): void;
  /** `SendRainbowRaceMiniGameExtState( { nowGame, state, p1, p2 }, data )`: the packet with texts (typing). */
  miniGameExtState(
    nowGame: number,
    state: number,
    p1: number,
    p2: number,
    data: readonly string[],
  ): void;
}

export function rainbowSenders(send: Send): RainbowSenders {
  const packet = (nowGame: number, state: number, p1: number, p2: number) =>
    create(MiniGamePacketSchema, { nowGame, state, param1: p1, param2: p2 });
  return {
    applicationOpenWnd() {
      // legacy-send: SendRainbowRaceApplicationOpenWnd
      send({
        case: 'rainbowRaceApplicationOpenWndReq',
        value: create(RainbowRaceApplicationOpenWndReqSchema, {}),
      });
    },
    application() {
      // legacy-send: SendRainbowRaceApplication
      send({ case: 'rainbowRaceApplication', value: create(RainbowRaceApplicationSchema, {}) });
    },
    prevRankingOpenWnd() {
      // legacy-send: SendRainbowRacePrevRankingOpenWnd
      send({
        case: 'rainbowRacePrevRankingOpenWndReq',
        value: create(RainbowRacePrevRankingOpenWndReqSchema, {}),
      });
    },
    reqFinish() {
      // legacy-send: SendRainbowRaceReqFinish
      send({ case: 'rainbowRaceReqFinish', value: create(RainbowRaceReqFinishSchema, {}) });
    },
    miniGameState(nowGame, state, p1, p2) {
      // legacy-send: SendRainbowRaceMiniGameState
      send({
        case: 'rainbowRaceMiniGamePacket',
        value: create(RainbowRaceMiniGamePacketSchema, {
          ext: false,
          packet: packet(nowGame, state, p1, p2),
          data: [],
        }),
      });
    },
    miniGameExtState(nowGame, state, p1, p2, data) {
      // legacy-send: SendRainbowRaceMiniGameExtState
      send({
        case: 'rainbowRaceMiniGamePacket',
        value: create(RainbowRaceMiniGamePacketSchema, {
          ext: true,
          packet: packet(nowGame, state, p1, p2),
          data: [...data],
        }),
      });
    },
  };
}
