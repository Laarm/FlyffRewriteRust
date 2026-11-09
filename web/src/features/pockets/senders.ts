// The sends of the pocket window (rule C9; `// legacy-send:` markers name the legacy send function, ref/99 App. B).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { AvailPocketSchema, MoveItemPocketSchema } from '../../gen/proto/flyff/v1/inventory_pb';

/** `-1`: the inventory as the source or the target of a `MoveItemPocket`. */
export const POCKET_BAG = -1;

export interface PocketSenders {
  /** `SendMoveItem_Pocket(from, objId, num, to)`; `-1` is the inventory. */
  move(from: number, objId: number, num: number, to: number): void;
  /** `SendAvailPocket(pocket, objId)`: activates bag `pocket` with a pocket item of the inventory. */
  avail(pocket: number, objId: number): void;
}

export function createPocketSenders(ctx: ClientContext): PocketSenders {
  const send = ctx.net.sender;
  return {
    move(from, objId, num, to) {
      // legacy-send: SendMoveItem_Pocket
      send.send({
        case: 'moveItemPocket',
        value: create(MoveItemPocketSchema, { pocketFrom: from, objId, num, pocketTo: to }),
      });
    },
    avail(pocket, objId) {
      // legacy-send: SendAvailPocket
      send.send({
        case: 'availPocket',
        value: create(AvailPocketSchema, { pocket, itemObjId: objId }),
      });
    },
  };
}
