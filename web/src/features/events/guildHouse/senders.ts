// The sends of the guild-house feature (rule C9: `// legacy-send: <SendFn>`): the three menu sends of the guild-house NPCs and `GuildHousePacketReq`, whose legacy sender is `CGuildHouseBase::SendClientToWorld` (no row of the csv,
// no marker). The session identifies the sender (VR-001).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { Vec3Schema } from '../../../gen/proto/flyff/v1/common_pb';
import {
  GhFntrInfoSchema,
  GuildHouseBuySchema,
  GuildHouseEnterSchema,
  GuildHouseGoOutSchema,
  GuildHousePacketReqSchema,
} from '../../../gen/proto/flyff/v1/housing_pb';
import { NULL_OBJ } from './slice';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The `GH_Fntr_Info` of a packet request. */
export interface PacketInfo {
  /** UPKEEP: the number of days. */
  readonly itemId: number;
  readonly setup: boolean;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  readonly ctrlObjId: number;
  /** Unix seconds. */
  readonly expireAt: number;
}

export interface GuildHouseSenders {
  /** `SendBuyGuildHouse()` (menu 147). */
  buy(): void;
  /** `SendGuildHouseEnter()` (menu 149). */
  enter(): void;
  /** `SendGuildHouseGoOut()` (menu 150). */
  goOut(): void;
  /** `SendClientToWorld( op, info, index )`: SETUP 2 / REMOVE 3 / RESET 4 / UPKEEP 6. */
  packet(op: number, index: number, info: PacketInfo): void;
}

/** An info with nothing but the item id (a REMOVE needs only the index; the days of an UPKEEP travel in `itemId`). */
export function bareInfo(itemId: number): PacketInfo {
  return { itemId, setup: false, pos: [0, 0, 0], angle: 0, ctrlObjId: NULL_OBJ, expireAt: 0 };
}

export function guildHouseSenders(send: Send): GuildHouseSenders {
  return {
    buy() {
      // legacy-send: SendBuyGuildHouse
      send({ case: 'guildHouseBuy', value: create(GuildHouseBuySchema) });
    },
    enter() {
      // legacy-send: SendGuildHouseEnter
      send({ case: 'guildHouseEnter', value: create(GuildHouseEnterSchema) });
    },
    goOut() {
      // legacy-send: SendGuildHouseGoOut
      send({ case: 'guildHouseGoOut', value: create(GuildHouseGoOutSchema) });
    },
    packet(op, index, info) {
      send({
        case: 'guildHousePacketReq',
        value: create(GuildHousePacketReqSchema, {
          op,
          index,
          info: create(GhFntrInfoSchema, {
            itemId: info.itemId,
            setup: info.setup,
            pos: create(Vec3Schema, { x: info.pos[0], y: info.pos[1], z: info.pos[2] }),
            angle: info.angle,
            ctrlObjId: info.ctrlObjId,
            expireAt: BigInt(Math.trunc(info.expireAt)),
          }),
        }),
      });
    },
  };
}
