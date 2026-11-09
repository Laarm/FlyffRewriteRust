// The client -> server sends of the item-specific use windows (Plan/45 §9.6, §18.3, §18.5; WP-CFEAT-15; rule C9: every send site carries its
// `// legacy-send:` marker). The sends of the other item windows (`DoUseItem`, `DoUseItemInput`, the pet feed ...) are the dispatch's
// (`features/inventory/dispatch/senders.ts`); the windows of this folder call those through the inventory feature.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { QuerySetPlayerNameReqSchema } from '../../gen/proto/flyff/v1/messenger_pb';
import {
  ChangeAttributeSchema,
  RandomOptionRetrySchema,
} from '../../gen/proto/flyff/v1/upgrade_pb';

export interface ItemUseWindowSenders {
  /** `SendChangeAttribute(item, scroll, attribute 1..5)`. */
  changeAttribute(itemObjId: number, scrollObjId: number, attribute: number): void;
  /** `SendRandomOptionRetry(item, scroll, line 1..3)`. */
  randomOptionRetry(itemObjId: number, scrollObjId: number, line: number): void;
  /** `SendQuerySetPlayerName(MAKELONG(scroll object id, 0), name)`: the character rename scroll (the answer `QuerySetPlayerNameNtf` is the messenger's, WP-CFEAT-39). */
  querySetPlayerName(scrollObjId: number, name: string): void;
}

export function createItemUseWindowSenders(ctx: ClientContext): ItemUseWindowSenders {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    changeAttribute(itemObjId, scrollObjId, attribute) {
      // legacy-send: SendChangeAttribute
      send({
        case: 'changeAttribute',
        value: create(ChangeAttributeSchema, {
          itemObjId,
          materialObjId: scrollObjId,
          attribute,
        }),
      });
    },
    randomOptionRetry(itemObjId, scrollObjId, line) {
      // legacy-send: SendRandomOptionRetry
      send({
        case: 'randomOptionRetry',
        value: create(RandomOptionRetrySchema, { itemObjId, scrollObjId, line }),
      });
    },
    querySetPlayerName(scrollObjId, name) {
      // legacy-send: SendQuerySetPlayerName
      send({
        case: 'querySetPlayerNameReq',
        value: create(QuerySetPlayerNameReqSchema, { data: scrollObjId & 0xffff, name }),
      });
    },
  };
}
