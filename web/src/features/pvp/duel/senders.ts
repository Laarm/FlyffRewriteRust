// The sends of the duel windows and the duel menu (rule C9). The ids on the wire are character ids (`m_idPlayer`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  DuelNoReqSchema,
  DuelPartyNoReqSchema,
  DuelPartyRequestReqSchema,
  DuelPartyYesSchema,
  DuelRequestReqSchema,
  DuelYesSchema,
} from '../../../gen/proto/flyff/v1/pvp_pb';

/** `SendDuelRequest(g_pPlayer, pFocusMover)`: challenge `dst` to a solo duel (`src` is ignored by the server, VR-001). */
export function sendDuelRequest(ctx: ClientContext, src: number, dst: number): void {
  // legacy-send: SendDuelRequest
  ctx.net.sender.send({
    case: 'duelRequestReq',
    value: create(DuelRequestReqSchema, { src, dst }),
  });
}

/** `SendDuelPartyRequest(g_pPlayer, pFocusMover)`: challenge the party of `dst` (both must lead their party). */
export function sendDuelPartyRequest(ctx: ClientContext, src: number, dst: number): void {
  // legacy-send: SendDuelPartyRequest
  ctx.net.sender.send({
    case: 'duelPartyRequestReq',
    value: create(DuelPartyRequestReqSchema, { src, dst }),
  });
}

/** `SendDuelYes(pSrc, g_pPlayer)`: accept the solo duel of the requester `src`. */
export function sendDuelYes(ctx: ClientContext, src: number): void {
  // legacy-send: SendDuelYes
  ctx.net.sender.send({ case: 'duelYes', value: create(DuelYesSchema, { src }) });
}

/** `SendDuelNo(pSrc)`: decline the solo duel of the requester `src`. */
export function sendDuelNo(ctx: ClientContext, src: number): void {
  // legacy-send: SendDuelNo
  ctx.net.sender.send({ case: 'duelNoReq', value: create(DuelNoReqSchema, { src }) });
}

/** `SendDuelPartyYes(pSrc, g_pPlayer)`: accept the party duel of the requesting leader `src`. */
export function sendDuelPartyYes(ctx: ClientContext, src: number): void {
  // legacy-send: SendDuelPartyYes
  ctx.net.sender.send({ case: 'duelPartyYes', value: create(DuelPartyYesSchema, { src }) });
}

/** `SendDuelPartyNo(pSrc)`: decline the party duel of the requesting leader `src`. */
export function sendDuelPartyNo(ctx: ClientContext, src: number): void {
  // legacy-send: SendDuelPartyNo
  ctx.net.sender.send({ case: 'duelPartyNoReq', value: create(DuelPartyNoReqSchema, { src }) });
}
