// The client -> server sends of the trade (Plan/45 §14; rule C9: every send site carries its `// legacy-send:` marker).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { ITYPE_ITEM } from '../../gen/defs/define';
import {
  ConfirmTradeCancelReqSchema,
  ConfirmTradeReqSchema,
  TradeCancelReqSchema,
  TradeConfirmSchema,
  TradeOkReqSchema,
  TradePullReqSchema,
  TradePutGoldReqSchema,
  TradePutReqSchema,
  TradeReqSchema,
} from '../../gen/proto/flyff/v1/trade_pb';

export interface TradeSenders {
  /** `SendConfirmTrade( pTrader )`: ask a player to trade. */
  confirmTrade(target: number): void;
  /** `SendConfirmTradeCancel( objid )`: decline a request. */
  confirmTradeCancel(requester: number): void;
  /** `SendTrade( pTrader )`: accept a request. */
  trade(requester: number): void;
  /** `SendTradePut( i, ITYPE_ITEM, objId, num )`. */
  put(slot: number, objId: number, num: number): void;
  /** `SendTradePull( i )`. */
  pull(slot: number): void;
  /** `SendTradePutGold( gold )`: the amount to ADD to the offer. */
  putGold(gold: number): void;
  /** `SendTradeOk()`. */
  ok(): void;
  /** `SendTradeConfirm()`. */
  confirm(): void;
  /** `SendTradeCancel()`: mode 0. */
  cancel(): void;
}

export function createTradeSenders(ctx: ClientContext): TradeSenders {
  const s = ctx.net.sender;
  return {
    confirmTrade(target) {
      // legacy-send: SendConfirmTrade
      s.send({ case: 'confirmTradeReq', value: create(ConfirmTradeReqSchema, { target }) });
    },
    confirmTradeCancel(requester) {
      // legacy-send: SendConfirmTradeCancel
      s.send({
        case: 'confirmTradeCancelReq',
        value: create(ConfirmTradeCancelReqSchema, { requester }),
      });
    },
    trade(requester) {
      // legacy-send: SendTrade
      s.send({ case: 'tradeReq', value: create(TradeReqSchema, { requester }) });
    },
    put(slot, objId, num) {
      // legacy-send: SendTradePut
      s.send({
        case: 'tradePutReq',
        value: create(TradePutReqSchema, { slot, itemType: ITYPE_ITEM, objId, num }),
      });
    },
    pull(slot) {
      // legacy-send: SendTradePull
      s.send({ case: 'tradePullReq', value: create(TradePullReqSchema, { slot }) });
    },
    putGold(gold) {
      // legacy-send: SendTradePutGold
      s.send({ case: 'tradePutGoldReq', value: create(TradePutGoldReqSchema, { gold }) });
    },
    ok() {
      // legacy-send: SendTradeOk
      s.send({ case: 'tradeOkReq', value: create(TradeOkReqSchema, {}) });
    },
    confirm() {
      // legacy-send: SendTradeConfirm
      s.send({ case: 'tradeConfirm', value: create(TradeConfirmSchema, {}) });
    },
    cancel() {
      // legacy-send: SendTradeCancel
      s.send({ case: 'tradeCancelReq', value: create(TradeCancelReqSchema, { mode: 0 }) });
    },
  };
}
