// The client -> server sends of the pet windows (Plan/45 §18.7, rule C9: every send site carries its `// legacy-send:` marker; WP-CFEAT-27). The pet FOOD of the
// inventory (`UsePetFeed`) and the pick-up pet awakening cancel (`PickupPetAwakeningCancel`) are sent by the item dispatch and the upgrade feature.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  TransformStuffItemSchema,
  TransformStuffSchema,
} from '../../gen/proto/flyff/v1/item_types_pb';
import {
  ClearPetNameSchema,
  MakePetFeedSchema,
  PetFilterSchema,
  PetReleaseReqSchema,
  PetTamerMiracleSchema,
  QuePetResurrectionSchema,
  TransformItemSchema,
  VisPetRemoveVisSchema,
  VisPetSwapVisSchema,
} from '../../gen/proto/flyff/v1/pet_pb';

const NULL_ID = 0xffff_ffff;

export interface PetSenders {
  /** `SendPetRelease()`: dismiss the summoned system pet. */
  petRelease(): void;
  /** `SendMakePetFeed( material, num, tool )`: `tool = null` is the NPC (`NULL_ID`). */
  makePetFeed(materialObjId: number, num: number, tool: number | null): void;
  /** `SendPetTamerMiracle( objId )`. */
  petTamerMiracle(objId: number): void;
  /** `SendQuePetResurrection( objId )`. */
  quePetResurrection(objId: number): void;
  /** `SendTransformItem( stuff )` of the egg transformer (`CTransformStuff( 0 )` with the ten eggs, one each). */
  transformItem(eggObjIds: readonly number[]): void;
  /** `SendRemoveVis( pos )`. */
  removeVis(pos: number): void;
  /** `SendSwapVis( a, b )`. */
  swapVis(a: number, b: number): void;
  /** `SendPlayerPetfilter( mask )`. */
  petFilter(mask: number): void;
  /** `SendClearPetName()` (`/ClearPetName`, `/cpn`). */
  clearPetName(): void;
}

export function createPetSenders(ctx: ClientContext): PetSenders {
  const send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]): void => {
    ctx.net.sender.send(msg);
  };
  return {
    petRelease() {
      // legacy-send: SendPetRelease
      send({ case: 'petReleaseReq', value: create(PetReleaseReqSchema, {}) });
    },
    makePetFeed(materialObjId, num, tool) {
      // legacy-send: SendMakePetFeed
      send({
        case: 'makePetFeed',
        value: create(MakePetFeedSchema, { materialObjId, toolObjId: tool ?? NULL_ID, num }),
      });
    },
    petTamerMiracle(objId) {
      // legacy-send: SendPetTamerMiracle
      send({ case: 'petTamerMiracle', value: create(PetTamerMiracleSchema, { objId }) });
    },
    quePetResurrection(objId) {
      // legacy-send: SendQuePetResurrection
      send({ case: 'quePetResurrection', value: create(QuePetResurrectionSchema, { objId }) });
    },
    transformItem(eggObjIds) {
      // legacy-send: SendTransformItem
      send({
        case: 'transformItem',
        value: create(TransformItemSchema, {
          stuff: create(TransformStuffSchema, {
            transformId: 0,
            items: eggObjIds.map((itemObjId) =>
              create(TransformStuffItemSchema, { itemObjId, num: 1 }),
            ),
          }),
        }),
      });
    },
    removeVis(pos) {
      // legacy-send: SendRemoveVis
      send({ case: 'visPetRemoveVis', value: create(VisPetRemoveVisSchema, { pos }) });
    },
    swapVis(a, b) {
      // legacy-send: SendSwapVis
      send({ case: 'visPetSwapVis', value: create(VisPetSwapVisSchema, { pos1: a, pos2: b }) });
    },
    petFilter(mask) {
      // legacy-send: SendPlayerPetfilter
      send({ case: 'petFilter', value: create(PetFilterSchema, { filter: mask >>> 0 }) });
    },
    clearPetName() {
      // legacy-send: SendClearPetName
      send({ case: 'clearPetName', value: create(ClearPetNameSchema, {}) });
    },
  };
}
