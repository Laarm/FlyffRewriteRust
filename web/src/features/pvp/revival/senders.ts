// The sends of the revival window (rule C9).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  ResurrectionCancelSchema,
  RevivalReqSchema,
  RevivalToLodestarReqSchema,
} from '../../../gen/proto/flyff/v1/combat_pb';

/** `SendHdr(PACKETTYPE_REVIVAL)`: revive in place (needs a resurrection scroll, the server checks). */
export function sendRevival(ctx: ClientContext): void {
  // legacy-send: SendHdr
  ctx.net.sender.send({ case: 'revivalReq', value: create(RevivalReqSchema, {}) });
}

/** `SendHdr(PACKETTYPE_REVIVAL_TO_LODESTAR)`: revive at the revival point of the nearest village. */
export function sendRevivalToLodestar(ctx: ClientContext): void {
  // legacy-send: SendHdr
  ctx.net.sender.send({
    case: 'revivalToLodestarReq',
    value: create(RevivalToLodestarReqSchema, {}),
  });
}

/** `SendResurrectionCancel()`: declines an open resurrection offer before the revival is chosen. */
export function sendResurrectionCancel(ctx: ClientContext): void {
  // legacy-send: SendResurrectionCancel
  ctx.net.sender.send({
    case: 'resurrectionCancel',
    value: create(ResurrectionCancelSchema, {}),
  });
}
