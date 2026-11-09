// The client -> server sends of the personal shop and its chat room (Plan/45 §15; rule C9: every send site carries its `// legacy-send:` marker).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { ITYPE_ITEM } from '../../gen/defs/define';
import {
  BuyPVendorItemSchema,
  ChattingReqSchema,
  CloseChattingRoomSchema,
  EnterChattingSchema,
  OpenChattingRoomSchema,
  PVendorCloseReqSchema,
  PVendorOpenReqSchema,
  QueryPVendorItemSchema,
  RegisterPVendorItemReqSchema,
  UnregisterPVendorItemReqSchema,
} from '../../gen/proto/flyff/v1/trade_pb';

export interface VendorSenders {
  /** `SendRegisterPVendorItem( slot, 0, objId, num, cost )`. */
  register(slot: number, objId: number, num: number, cost: number): void;
  /** `SendUnregisterPVendorItem( slot )`. */
  unregister(slot: number): void;
  /** `SendPVendorOpen( title )`. */
  open(title: string): void;
  /** `SendPVendorClose( vendor )`: the own object id closes the own shop, another leaves a visited one. */
  close(vendor: number): void;
  /** `SendQueryPVendorItem( vendor )`. */
  query(vendor: number): void;
  /** `SendBuyPVendorItem( vendor, slot, itemId, num )`. */
  buy(vendor: number, slot: number, itemId: number, num: number): void;
  /** `SendEnterChattingRoom( idPlayer of the vendor )`. */
  enterChatting(room: number): void;
  /** `SendChatting( text )`. */
  chatting(text: string): void;
  /** `SendOpenChattingRoom()`. */
  openChattingRoom(): void;
  /** `SendCloseChattingRoom()`. */
  closeChattingRoom(): void;
}

export function createVendorSenders(ctx: ClientContext): VendorSenders {
  const s = ctx.net.sender;
  return {
    register(slot, objId, num, cost) {
      // legacy-send: SendRegisterPVendorItem
      s.send({
        case: 'registerPVendorItemReq',
        value: create(RegisterPVendorItemReqSchema, {
          slot,
          itemType: ITYPE_ITEM,
          objId,
          num,
          cost,
        }),
      });
    },
    unregister(slot) {
      // legacy-send: SendUnregisterPVendorItem
      s.send({
        case: 'unregisterPVendorItemReq',
        value: create(UnregisterPVendorItemReqSchema, { slot }),
      });
    },
    open(title) {
      // legacy-send: SendPVendorOpen
      s.send({ case: 'pVendorOpenReq', value: create(PVendorOpenReqSchema, { title }) });
    },
    close(vendor) {
      // legacy-send: SendPVendorClose
      s.send({ case: 'pVendorCloseReq', value: create(PVendorCloseReqSchema, { vendor }) });
    },
    query(vendor) {
      // legacy-send: SendQueryPVendorItem
      s.send({ case: 'queryPVendorItem', value: create(QueryPVendorItemSchema, { vendor }) });
    },
    buy(vendor, slot, itemId, num) {
      // legacy-send: SendBuyPVendorItem
      s.send({
        case: 'buyPVendorItem',
        value: create(BuyPVendorItemSchema, { vendor, slot, itemId, num }),
      });
    },
    enterChatting(room) {
      // legacy-send: SendEnterChattingRoom
      s.send({ case: 'enterChatting', value: create(EnterChattingSchema, { room }) });
    },
    chatting(text) {
      // legacy-send: SendChatting
      s.send({ case: 'chattingReq', value: create(ChattingReqSchema, { text }) });
    },
    openChattingRoom() {
      // legacy-send: SendOpenChattingRoom
      s.send({ case: 'openChattingRoom', value: create(OpenChattingRoomSchema, {}) });
    },
    closeChattingRoom() {
      // legacy-send: SendCloseChattingRoom
      s.send({ case: 'closeChattingRoom', value: create(CloseChattingRoomSchema, {}) });
    },
  };
}
