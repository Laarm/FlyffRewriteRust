// The send of the stat allocation (rule C9).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { ModifyStatusSchema } from '../../../gen/proto/flyff/v1/character_pb';
import type { StatDelta } from '../../../ui/windows/char/ports';

/** `SendModifyStatus(str, sta, dex, int)`; the answer is `SetState` (self only). */
export function sendModifyStatus(ctx: ClientContext, d: StatDelta): void {
  // legacy-send: SendModifyStatus
  ctx.net.sender.send({
    case: 'modifyStatus',
    value: create(ModifyStatusSchema, { str: d.str, sta: d.sta, dex: d.dex, int: d.int }),
  });
}
