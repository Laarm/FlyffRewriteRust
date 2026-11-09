// The send of the resurrection confirm (rule C9). `ResurrectionCancel` is the revival feature's (`../revival/senders.ts`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { ResurrectionOkSchema } from '../../../gen/proto/flyff/v1/combat_pb';

/** `SendResurrectionOK()`: accept the Resurrection skill of another player. */
export function sendResurrectionOk(ctx: ClientContext): void {
  // legacy-send: SendResurrectionOK
  ctx.net.sender.send({ case: 'resurrectionOk', value: create(ResurrectionOkSchema, {}) });
}
