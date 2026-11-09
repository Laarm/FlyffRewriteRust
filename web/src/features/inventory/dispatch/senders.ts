// The sends of the item dispatch (Plan/45 §9; WP-CFEAT-13, -14; rule C9: every send site carries its `// legacy-send:` marker). One
// small function per legacy `Send*`; the router and the upgrade cursor decide WHEN. `doUseItem` and `doEquip` are the inventory's.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { OpenBankWndSchema } from '../../../gen/proto/flyff/v1/bank_pb';
import { Vec3Schema } from '../../../gen/proto/flyff/v1/common_pb';
import {
  CreateMonsterSchema,
  DoEscapeSchema,
  DoUseItemInputSchema,
  DoUseItemTargetSchema,
  ReturnScrollReqSchema,
} from '../../../gen/proto/flyff/v1/inventory_pb';
import {
  AngelMaterialSchema,
  CreateAngelSchema,
  FeedPocketInactiveSchema,
  UsePetFeedSchema,
} from '../../../gen/proto/flyff/v1/pet_pb';
import {
  EnchantSchema,
  PiercingReqSchema,
  RandomScrollSchema,
  UltimateEnchantWeaponSchema,
} from '../../../gen/proto/flyff/v1/upgrade_pb';
import { StateModeReqSchema } from '../../../gen/proto/flyff/v1/character_pb';
import { type InventorySenders, itemData } from '../senders';
import type { DispatchSenders } from './env';

/** `STATE_BASEMOTION_MODE` / `STATEMODE_BASEMOTION_CANCEL` of `StateModeReq`. */
export const STATE_BASEMOTION_MODE = 4;
export const STATEMODE_BASEMOTION_CANCEL = 2;

/** The extra sends the dispatch's neighbours (the buff strip, the escape motion) use. */
export interface ItemUseSenders extends DispatchSenders {
  /** `SendStateModeBaseMotion(STATEMODE_BASEMOTION_CANCEL)`: the cast of a timed item is cancelled. */
  cancelCast(): void;
  /** `SendHdr(PACKETTYPE_DO_ESCAPE)`: the free escape command. */
  doEscape(): void;
  /** `SendFeedPocketInactive`: the double click on the feed-pocket buff icon. */
  feedPocketInactive(): void;
  /** `SendCreateAngel(str)`: the materials of the summon window as (object id, count) pairs. */
  createAngel(materials: readonly { objId: number; count: number }[]): void;
}

export function createDispatchSenders(ctx: ClientContext, base: InventorySenders): ItemUseSenders {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    doUseItem: (objId, target, part) => {
      base.doUseItem(objId, target, part);
    },
    doEquip: (objId, part) => {
      base.doEquip(objId, part);
    },
    doUseItemInput(objId, input) {
      // legacy-send: SendDoUseItemInput
      send({
        case: 'doUseItemInput',
        value: create(DoUseItemInputSchema, { data: itemData(objId), input }),
      });
    },
    enchant(itemObjId, materialObjId) {
      // legacy-send: SendEnchant
      send({ case: 'enchant', value: create(EnchantSchema, { itemObjId, materialObjId }) });
    },
    piercing(itemObjId, cardObjId) {
      // legacy-send: SendPiercing
      send({ case: 'piercingReq', value: create(PiercingReqSchema, { itemObjId, cardObjId }) });
    },
    ultimateEnchantWeapon(weaponObjId, shiningObjId) {
      // legacy-send: SendUltimateEnchantWeapon
      send({
        case: 'ultimateEnchantWeapon',
        value: create(UltimateEnchantWeaponSchema, { weaponObjId, shiningObjId }),
      });
    },
    doUseItemTarget(materialObjId, targetObjId) {
      // legacy-send: SendDoUseItemTarget
      send({
        case: 'doUseItemTarget',
        value: create(DoUseItemTargetSchema, { materialObjId, targetObjId }),
      });
    },
    randomScroll(itemObjId, scrollObjId) {
      // legacy-send: SendRandomScroll
      send({ case: 'randomScroll', value: create(RandomScrollSchema, { itemObjId, scrollObjId }) });
    },
    returnScrollReq(select) {
      // legacy-send: SendReturnScroll
      send({ case: 'returnScrollReq', value: create(ReturnScrollReqSchema, { select }) });
    },
    openBankWnd(objId, itemId) {
      // legacy-send: SendOpenBankWnd
      send({ case: 'openBankWnd', value: create(OpenBankWndSchema, { objId, itemId }) });
    },
    usePetFeed(feedObjId) {
      // legacy-send: SendUsePetFeed
      send({ case: 'usePetFeed', value: create(UsePetFeedSchema, { feedObjId }) });
    },
    createMonster(objId, pos) {
      // legacy-send: SendCreateMonster
      send({
        case: 'createMonster',
        value: create(CreateMonsterSchema, {
          data: itemData(objId),
          pos: create(Vec3Schema, { x: pos[0], y: pos[1], z: pos[2] }),
        }),
      });
    },
    cancelCast() {
      // legacy-send: SendStateModeCancel
      send({
        case: 'stateModeReq',
        value: create(StateModeReqSchema, {
          stateMode: STATE_BASEMOTION_MODE,
          flag: STATEMODE_BASEMOTION_CANCEL,
        }),
      });
    },
    doEscape() {
      // legacy-send: SendDoEscape
      send({ case: 'doEscape', value: create(DoEscapeSchema, {}) });
    },
    createAngel(materials) {
      // legacy-send: SendCreateAngel
      send({
        case: 'createAngel',
        value: create(CreateAngelSchema, {
          materials: materials.map((m) =>
            create(AngelMaterialSchema, { objId: m.objId, count: m.count }),
          ),
        }),
      });
    },
    feedPocketInactive() {
      // legacy-send: SendFeedPocketInactive
      send({ case: 'feedPocketInactive', value: create(FeedPocketInactiveSchema, {}) });
    },
  };
}
