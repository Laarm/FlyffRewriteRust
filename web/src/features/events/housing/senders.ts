// The sends of the housing feature (rule C9: `// legacy-send: <SendFn>`): the five functions of the mini room. The session identifies the sender (VR-001), so the leading own-id fields of the legacy packets are gone.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  HousingGoOutSchema,
  HousingInfoSchema,
  HousingReqVisitableListSchema,
  HousingSetupFurnitureReqSchema,
  HousingSetVisitAllowReqSchema,
  HousingVisitRoomSchema,
} from '../../../gen/proto/flyff/v1/housing_pb';
import { Vec3Schema } from '../../../gen/proto/flyff/v1/common_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The `HOUSINGINFO` a setup request carries. */
export interface SetupRequest {
  readonly itemId: number;
  readonly setup: boolean;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  /** Unix seconds; the server keeps its own remaining time (0). */
  readonly expireAt: number;
}

export interface HousingSenders {
  /** `SendHousingReqSetupFurniture( housingInfo )`: install or remove a furniture of the own room. */
  reqSetup(info: SetupRequest): void;
  /** `SendHousingReqSetVisitAllow( idPlayer, allow )`: allow or forbid a friend to visit. */
  reqSetVisitAllow(target: number, allow: boolean): void;
  /** `SendHousingVisitRoom( owner )`: enter a room (the own one included). */
  visitRoom(owner: number): void;
  /** `SendHousingReqVisitableList()`. */
  reqVisitableList(): void;
  /** `SendHousingGoOut()`. */
  goOut(): void;
}

export function housingSenders(send: Send): HousingSenders {
  return {
    reqSetup(info) {
      // legacy-send: SendHousingReqSetupFurniture
      send({
        case: 'housingSetupFurnitureReq',
        value: create(HousingSetupFurnitureReqSchema, {
          info: create(HousingInfoSchema, {
            itemId: info.itemId,
            setup: info.setup,
            expireAt: BigInt(Math.trunc(info.expireAt)),
            pos: create(Vec3Schema, { x: info.pos[0], y: info.pos[1], z: info.pos[2] }),
            angle: info.angle,
          }),
        }),
      });
    },
    reqSetVisitAllow(target, allow) {
      // legacy-send: SendHousingReqSetVisitAllow
      send({
        case: 'housingSetVisitAllowReq',
        value: create(HousingSetVisitAllowReqSchema, { target, allow }),
      });
    },
    visitRoom(owner) {
      // legacy-send: SendHousingVisitRoom
      send({ case: 'housingVisitRoom', value: create(HousingVisitRoomSchema, { owner }) });
    },
    reqVisitableList() {
      // legacy-send: SendHousingReqVisitableList
      send({
        case: 'housingReqVisitableList',
        value: create(HousingReqVisitableListSchema),
      });
    },
    goOut() {
      // legacy-send: SendHousingGoOut
      send({ case: 'housingGoOut', value: create(HousingGoOutSchema) });
    },
  };
}
