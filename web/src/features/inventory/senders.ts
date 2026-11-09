// The client -> server sends of the inventory window (Plan/45 §9; rule C9: every send site
// carries its `// legacy-send:` marker). One small function per legacy `Send*`; the callers (flows, actions) decide WHEN.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { ITYPE_ITEM } from '../../gen/defs/define';
import {
  HideCoatReqSchema,
  OptionEnableRenderMaskSchema,
} from '../../gen/proto/flyff/v1/character_pb';
import { Vec3Schema } from '../../gen/proto/flyff/v1/common_pb';
import {
  DoEquipReqSchema,
  DoUseItemSchema,
  DropItemSchema,
  MoveItemReqSchema,
  RemoveInvenItemSchema,
  SortInventorySchema,
} from '../../gen/proto/flyff/v1/inventory_pb';

/** `MAKELONG(ITYPE_ITEM, objId)`: the object id sits in the high word. */
export const itemData = (objId: number): number => ((objId << 16) | ITYPE_ITEM) >>> 0;

/** The senders of a context. */
export interface InventorySenders {
  /** `SendDoUseItem(MAKELONG(ITYPE_ITEM, objId), target, part)`. */
  doUseItem(objId: number, target: number, part: number): void;
  /** `SendDoEquip(item, part)`: `part = -1` wears the item. */
  doEquip(objId: number, part: number): void;
  /** `SendMoveItem(0, src, dst)`: bag SLOT indices. */
  moveItem(src: number, dst: number): void;
  /** `SendDropItem(0, objId, num, pos)`. */
  dropItem(objId: number, num: number, pos: readonly [number, number, number]): void;
  /** `SendRemoveItem(objId, num)`. */
  removeItem(objId: number, num: number): void;
  /** `SendHdr(PACKETTYPE_SORT_INVENTORY)`. */
  sort(): void;
  /** `SendHideCoat(position, hide)`. */
  hideCoat(position: number, hide: boolean): void;
  /** `SendOptionEnableRenderMask(enable)`. */
  renderMask(enable: boolean): void;
}

export function createInventorySenders(ctx: ClientContext): InventorySenders {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    doUseItem(objId, target, part) {
      // legacy-send: SendDoUseItem
      send({
        case: 'doUseItem',
        value: create(DoUseItemSchema, { data: itemData(objId), target: target >>> 0, part }),
      });
    },
    doEquip(objId, part) {
      // legacy-send: SendDoEquip
      send({ case: 'doEquipReq', value: create(DoEquipReqSchema, { objId, part }) });
    },
    moveItem(src, dst) {
      // legacy-send: SendMoveItem
      send({
        case: 'moveItemReq',
        value: create(MoveItemReqSchema, { itemType: 0, srcSlot: src, dstSlot: dst }),
      });
    },
    dropItem(objId, num, pos) {
      // legacy-send: SendDropItem
      send({
        case: 'dropItem',
        value: create(DropItemSchema, {
          itemType: 0,
          objId,
          num,
          pos: create(Vec3Schema, { x: pos[0], y: pos[1], z: pos[2] }),
        }),
      });
    },
    removeItem(objId, num) {
      // legacy-send: SendRemoveItem
      send({ case: 'removeInvenItem', value: create(RemoveInvenItemSchema, { objId, num }) });
    },
    sort() {
      // legacy-send: SendHdr
      send({ case: 'sortInventory', value: create(SortInventorySchema, {}) });
    },
    hideCoat(position, hide) {
      // legacy-send: SendHideCoat
      send({ case: 'hideCoatReq', value: create(HideCoatReqSchema, { position, hide }) });
    },
    renderMask(enable) {
      // legacy-send: SendOptionEnableRenderMask
      send({
        case: 'optionEnableRenderMask',
        value: create(OptionEnableRenderMaskSchema, { enable }),
      });
    },
  };
}
