// The sends of the quest windows (rule C9: `// legacy-send: <SendFn>`; Plan/45 §28.1, 20 §9.10; WP-CFEAT-42). The session identifies the sender (VR-001).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  QuestCheckSchema,
  QuestHelperReqNpcPosSchema,
  RemoveQuestReqSchema,
} from '../../gen/proto/flyff/v1/quest_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

export interface QuestSenders {
  /** `SendCheckedQuestId( id, check )`: tick or untick a quest for the quick-info list. */
  check(questId: number, check: boolean): void;
  /** `SendRemoveQuest( id )`: cancel a quest (also the legacy request after a timeout). */
  remove(questId: number): void;
  /** `SendReqNPCPos( key )`: where does the quest NPC stand? */
  reqNpcPos(characterKey: string): void;
}

export function questSenders(send: Send): QuestSenders {
  return {
    check(questId, check) {
      // legacy-send: SendCheckedQuestId
      send({ case: 'questCheck', value: create(QuestCheckSchema, { questId, check }) });
    },
    remove(questId) {
      // legacy-send: SendRemoveQuest
      send({ case: 'removeQuestReq', value: create(RemoveQuestReqSchema, { questId }) });
    },
    reqNpcPos(characterKey) {
      // legacy-send: SendReqNPCPos
      send({
        case: 'questHelperReqNpcPos',
        value: create(QuestHelperReqNpcPosSchema, { characterKey }),
      });
    },
  };
}
