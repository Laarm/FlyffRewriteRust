// The sends of the motions (rule C9).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { MotionReqSchema } from '../../gen/proto/flyff/v1/movement_pb';

/** `SendMotion(OBJMSG_SITDOWN | OBJMSG_STANDUP)`; the server answers `MotionNtf` and owns the sitting regeneration. */
export function sendMotion(ctx: ClientContext, objMsg: number): void {
  // legacy-send: SendMotion
  ctx.net.sender.send({ case: 'motionReq', value: create(MotionReqSchema, { objMsg }) });
}
