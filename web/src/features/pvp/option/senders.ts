// The send of the PvP option (rule C9).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { ChangeModeSchema } from '../../../gen/proto/flyff/v1/character_pb';

/**
 * `SendChangePKPVPMode(dwMode, nFlag)`: `flag` 0 / 1 set / clear `FREEPK_MODE`, 2 / 3 set / clear `PVPCONFIRM_MODE`. The server reads the
 * flag only (the `mode` word is ignored) and answers `ModifyMode`.
 */
export function sendChangeMode(ctx: ClientContext, mode: number, flag: number): void {
  // legacy-send: SendChangePKPVPMode
  ctx.net.sender.send({ case: 'changeMode', value: create(ChangeModeSchema, { mode, flag }) });
}
