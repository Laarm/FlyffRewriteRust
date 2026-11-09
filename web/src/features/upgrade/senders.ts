// The client -> server sends of the upgrade windows (Plan/45 §18.2-§18.5; WP-CFEAT-24, WP-CFEAT-25; rule C9: every send site carries its `// legacy-send:` marker).
// The legacy cursor upgrades (`Enchant`, `RandomScroll`, `DoUseItemTarget`) are the inventory dispatch's; the sends of the item-use windows (`ChangeAttribute`,
// `RandomOptionRetry`) are the `itemuse` feature's.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  AwakeningSchema,
  BlessednessCancelSchema,
  CommercialElemReqSchema,
  ItemSelectAwakeningValueReqSchema,
  RemoveAttributeReqSchema,
  RemoveItemLevelDownSchema,
  SmeltSafetyReqSchema,
} from '../../gen/proto/flyff/v1/upgrade_pb';
import { PickupPetAwakeningCancelSchema } from '../../gen/proto/flyff/v1/pet_pb';
import type { SmeltSend } from './safeSmelt/types';

export interface UpgradeSenders {
  /** `SendNewSmelt( item, material, protection, scroll )` / `SendSmeltSafety(...)`: ONE attempt of the safe upgrade. */
  smeltSafety(msg: SmeltSend): void;
  /** `SendRemoveAttribute( item )`. */
  removeAttribute(objId: number): void;
  /** `SendCommercialElem( item, stone )`. */
  commercialElem(itemObjId: number, stoneObjId: number): void;
  /** `SendAwakening( item )`. */
  awakening(objId: number): void;
  /** `SendBlessednessCancel( item )`. */
  blessednessCancel(objId: number): void;
  /** `SendSelectedAwakeningValue( item, serial, 1 | 2 )`. */
  selectAwakening(objId: number, serial: bigint, flag: 1 | 2): void;
  /** `SendRemoveItemLevelDown( item )`. */
  removeItemLevelDown(objId: number): void;
  /** `SendPickupPetAwakeningCancel( item )`. */
  pickupPetAwakeningCancel(objId: number): void;
}

export function createUpgradeSenders(ctx: ClientContext): UpgradeSenders {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    smeltSafety(m) {
      // legacy-send: SendNewSmelt, SendSmeltSafety
      send({
        case: 'smeltSafetyReq',
        value: create(SmeltSafetyReqSchema, {
          itemObjId: m.item,
          materialObjId: m.material,
          protectionObjId: m.protection,
          smeltScrollObjId: m.scroll,
        }),
      });
    },
    removeAttribute(objId) {
      // legacy-send: SendRemoveAttribute
      send({
        case: 'removeAttributeReq',
        value: create(RemoveAttributeReqSchema, { itemObjId: objId }),
      });
    },
    commercialElem(itemObjId, stoneObjId) {
      // legacy-send: SendCommercialElem
      send({
        case: 'commercialElemReq',
        value: create(CommercialElemReqSchema, { itemObjId, stoneObjId }),
      });
    },
    awakening(objId) {
      // legacy-send: SendAwakening
      send({ case: 'awakening', value: create(AwakeningSchema, { itemObjId: objId }) });
    },
    blessednessCancel(objId) {
      // legacy-send: SendBlessednessCancel
      send({
        case: 'blessednessCancel',
        value: create(BlessednessCancelSchema, { itemObjId: objId }),
      });
    },
    selectAwakening(objId, serial, flag) {
      // legacy-send: SendSelectedAwakeningValue
      send({
        case: 'itemSelectAwakeningValueReq',
        value: create(ItemSelectAwakeningValueReqSchema, {
          itemObjId: objId,
          serial,
          selectFlag: flag,
        }),
      });
    },
    removeItemLevelDown(objId) {
      // legacy-send: SendRemoveItemLevelDown
      send({
        case: 'removeItemLevelDown',
        value: create(RemoveItemLevelDownSchema, { itemObjId: objId }),
      });
    },
    pickupPetAwakeningCancel(objId) {
      // legacy-send: SendPickupPetAwakeningCancel
      send({
        case: 'pickupPetAwakeningCancel',
        value: create(PickupPetAwakeningCancelSchema, { objId }),
      });
    },
  };
}
