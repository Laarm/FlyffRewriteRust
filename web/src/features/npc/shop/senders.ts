// The sends of the shop windows (rule C9; `// legacy-send:` markers name the legacy send function, ref/99 App. B).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  BuyChipItemSchema,
  BuyItemSchema,
  ChangeFaceReqSchema,
  CloseShopWndSchema,
  OpenShopWndReqSchema,
  QuickSellSchema,
  SellItemSchema,
  SetHairReqSchema,
} from '../../../gen/proto/flyff/v1/shop_pb';

export interface ShopSenders {
  openShop(npcObjId: number): void;
  closeShop(): void;
  buy(tab: number, objId: number, num: number, itemId: number): void;
  buyChip(tab: number, objId: number, num: number, itemId: number): void;
  sell(objId: number, num: number): void;
  quickSell(): void;
  setHair(hair: number, r: number, g: number, b: number, useCoupon: boolean): void;
  changeFace(face: number, cost: number, useCoupon: boolean): void;
}

export function createShopSenders(ctx: ClientContext): ShopSenders {
  const send = ctx.net.sender;
  return {
    openShop(npcObjId) {
      // legacy-send: SendOpenShopWnd
      send.send({ case: 'openShopWndReq', value: create(OpenShopWndReqSchema, { npcObjId }) });
    },
    closeShop() {
      // legacy-send: SendCloseShopWnd
      send.send({ case: 'closeShopWnd', value: create(CloseShopWndSchema, {}) });
    },
    buy(tab, objId, num, itemId) {
      // legacy-send: SendBuyItem
      send.send({ case: 'buyItem', value: create(BuyItemSchema, { tab, objId, num, itemId }) });
    },
    buyChip(tab, objId, num, itemId) {
      // legacy-send: SendBuyChipItem
      send.send({
        case: 'buyChipItem',
        value: create(BuyChipItemSchema, { tab, objId, num, itemId }),
      });
    },
    sell(objId, num) {
      // legacy-send: SendSellItem
      send.send({ case: 'sellItem', value: create(SellItemSchema, { objId, num }) });
    },
    quickSell() {
      // legacy-send: SendQuickSell
      send.send({ case: 'quickSell', value: create(QuickSellSchema, {}) });
    },
    setHair(hair, r, g, b, useCoupon) {
      // legacy-send: SendSetHair
      send.send({
        case: 'setHairReq',
        value: create(SetHairReqSchema, { hair, r, g, b, useCoupon }),
      });
    },
    changeFace(face, cost, useCoupon) {
      // legacy-send: SendChangeFace
      send.send({
        case: 'changeFaceReq',
        value: create(ChangeFaceReqSchema, { face, cost, useCoupon }),
      });
    },
  };
}
