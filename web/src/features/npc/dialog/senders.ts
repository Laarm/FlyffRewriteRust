// The send of the dialog window (rule C9; `// legacy-send:` markers name the legacy send function, ref/99 App. B).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { ScriptDlgSchema } from '../../../gen/proto/flyff/v1/npc_pb';

/**
 * `SendScriptDialogReq(idNpc, key, param, quest, ...)`: an empty key opens the conversation (`#init`); the server answers with
 * ONE `DialogOps` and accepts only a key it offered (VR-060). The legacy trailing arguments (the NPC and the player id) are
 * dropped (the session names both).
 */
export function sendScriptDlg(
  ctx: ClientContext,
  npcObjId: number,
  key: string,
  param: number,
  questId: number,
): void {
  // legacy-send: SendScriptDialogReq
  ctx.net.sender.send({
    case: 'scriptDlg',
    value: create(ScriptDlgSchema, { npcObjId, key, param, questId }),
  });
}
