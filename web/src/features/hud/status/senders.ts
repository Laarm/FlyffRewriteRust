// The send of the status window (rule C9).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { ExpStopSchema } from '../../../gen/proto/flyff/v1/character_pb';

/** `SendExpStop()`: toggles `MODE_EXPUP_STOP`; the server answers with a text line and `ModifyMode`. */
export function sendExpStop(ctx: ClientContext): void {
  // legacy-send: SendExpStop
  ctx.net.sender.send({ case: 'expStop', value: create(ExpStopSchema, {}) });
}
