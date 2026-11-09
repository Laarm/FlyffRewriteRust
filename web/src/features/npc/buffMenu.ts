// The NPC buff menu entry (Plan/45 §11.2, Plan/20 §8.5, Plan/17 §7.11; tests-m4a): `MMI_NPC_BUFF` on an NPC that offers buffs
// (the helpers of Flaris) sends `NpcBuff{character_key}` at once, as `CWndWorld::OnCommand(MMI_NPC_BUFF)` does
// (`g_DPlay.SendNPCBuff(pFocusMover->GetCharacterKey())`). The server checks the range of any buff NPC (VR-012) and answers with
// `SetSkillState` per buff, which the buff feature shows in the strip; out of range it says nothing.
//
// No window belongs to this entry, so no window feature registered it before the M4a demo found the click on a helper doing
// nothing (the menu logged "no window yet").

import { create } from '@bufbuild/protobuf';
import { MMI_NPC_BUFF } from '../../gen/defs/neuz';
import { NpcBuffSchema } from '../../gen/proto/flyff/v1/npc_pb';
import type { FeatureContext } from '../common/module';
import type { MenuFeature } from './menuModule';

/** The longest character key the wire carries (`char[64]` of the legacy request). */
const MAX_KEY = 63;

/** Registers the `MMI_NPC_BUFF` action; returns the unregister function. */
export function registerNpcBuff(fc: FeatureContext, menu: MenuFeature): () => void {
  return menu.registerAction(MMI_NPC_BUFF, ({ npcKey }) => {
    if (npcKey === null || npcKey === '' || npcKey.length > MAX_KEY) return;
    // legacy-send: SendNPCBuff
    fc.app.net.sender.send({
      case: 'npcBuff',
      value: create(NpcBuffSchema, { characterKey: npcKey }),
    });
  });
}
