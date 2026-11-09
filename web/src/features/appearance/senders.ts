// The client -> server sends of the appearance windows (Plan/45 §18.4; WP-CFEAT-15; rule C9: every send site carries its `// legacy-send:`
// marker). The sex-change scroll sends the dispatch's `DoUseItem` (through the inventory feature); the beauty shops' sends are the NPC feature's.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { ItemTransySchema } from '../../gen/proto/flyff/v1/upgrade_pb';

export interface AppearanceSenders {
  /** `SendItemTransy(armour, scroll, change id, TRUE)`: the cash path (the penya path is compiled out). */
  itemTransy(itemObjId: number, scrollObjId: number, changeItemId: number): void;
}

export function createAppearanceSenders(ctx: ClientContext): AppearanceSenders {
  return {
    itemTransy(itemObjId, scrollObjId, changeItemId) {
      // legacy-send: SendItemTransy
      ctx.net.sender.send({
        case: 'itemTransy',
        value: create(ItemTransySchema, { itemObjId, scrollObjId, changeItemId, cash: true }),
      });
    },
  };
}
