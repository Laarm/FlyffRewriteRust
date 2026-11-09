// The client -> server sends of the ultimate-weapon, piercing and item-transy windows (Plan/45 §18.6; WP-CFEAT-26; rule C9: every send site carries
// its `// legacy-send:` marker). The upgrade cursor's `UltimateEnchantWeapon` and `PiercingReq` and the item transy are sent by the dispatch and the
// appearance feature (`features/inventory/dispatch/senders.ts`, `features/appearance/senders.ts`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  PiercingRemoveSchema,
  PiercingSizeSchema,
  UltimateMakeGemSchema,
  UltimateMakeItemSchema,
  UltimateRemoveGemSchema,
  UltimateSetGemSchema,
  UltimateTransWeaponSchema,
} from '../../gen/proto/flyff/v1/upgrade_pb';

/** `NULL_ID`: "no item" (the protection of a piercing size is optional). */
export const NO_ITEM = 0xffff_ffff;
/** `MAX_JEWEL`: `UltimateMakeItem` carries exactly ten stack ids. */
export const MIX_IDS = 10;

export interface UltimateSenders {
  /** `SendUltimateMakeItem( objids[MAX_JEWEL] )`: the first five are Orichalcum stacks, the last five moonstone stacks. */
  makeItem(objIds: readonly number[]): void;
  /** `SendUltimateMakeGem( weapon )`. */
  makeGem(weaponObjId: number): void;
  /** `SendUltimateTransWeapon( weapon, jewel, shining )`. */
  transWeapon(weaponObjId: number, gemObjId: number, shiningObjId: number): void;
  /** `SendUltimateSetGem( weapon, jewel )`. */
  setGem(weaponObjId: number, gemObjId: number): void;
  /** `SendUltimateRemoveGem( weapon, moonstone )`. */
  removeGem(weaponObjId: number, moonstoneObjId: number): void;
  /** `SendPiercingSize( item, moonstone, protection )`; `protectionObjId` is {@link NO_ITEM} without a protection scroll. */
  piercingSize(itemObjId: number, moonstoneObjId: number, protectionObjId?: number): void;
  /** `SendPiercingRemove( item )`. */
  piercingRemove(itemObjId: number): void;
}

export function createUltimateSenders(ctx: ClientContext): UltimateSenders {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    makeItem(objIds) {
      // the wire list has exactly ten entries (`OBJID itemobjid[MAX_JEWEL]`): the rest of a short list is zero like the original's array
      const ids = Array.from({ length: MIX_IDS }, (_, i) => objIds[i] ?? 0);
      // legacy-send: SendUltimateMakeItem
      send({
        case: 'ultimateMakeItem',
        value: create(UltimateMakeItemSchema, { itemObjIds: ids }),
      });
    },
    makeGem(weaponObjId) {
      // legacy-send: SendUltimateMakeGem
      send({ case: 'ultimateMakeGem', value: create(UltimateMakeGemSchema, { weaponObjId }) });
    },
    transWeapon(weaponObjId, gemObjId, shiningObjId) {
      // legacy-send: SendUltimateTransWeapon
      send({
        case: 'ultimateTransWeapon',
        value: create(UltimateTransWeaponSchema, { weaponObjId, gemObjId, shiningObjId }),
      });
    },
    setGem(weaponObjId, gemObjId) {
      // legacy-send: SendUltimateSetGem
      send({
        case: 'ultimateSetGem',
        value: create(UltimateSetGemSchema, { weaponObjId, gemObjId }),
      });
    },
    removeGem(weaponObjId, moonstoneObjId) {
      // legacy-send: SendUltimateRemoveGem
      send({
        case: 'ultimateRemoveGem',
        value: create(UltimateRemoveGemSchema, { weaponObjId, moonstoneObjId }),
      });
    },
    piercingSize(itemObjId, moonstoneObjId, protectionObjId = NO_ITEM) {
      // legacy-send: SendPiercingSize
      send({
        case: 'piercingSize',
        value: create(PiercingSizeSchema, { itemObjId, moonstoneObjId, protectionObjId }),
      });
    },
    piercingRemove(itemObjId) {
      // legacy-send: SendPiercingRemove
      send({ case: 'piercingRemove', value: create(PiercingRemoveSchema, { itemObjId }) });
    },
  };
}
