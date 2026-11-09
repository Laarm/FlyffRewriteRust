// The sends of the bank windows (rule C9; `// legacy-send:` markers name the legacy send function, ref/99 App. B).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  ChangeBankPassReqSchema,
  CloseBankWndSchema,
  ConfirmBankSchema,
  GetGoldBackSchema,
  GetItemBackSchema,
  OpenBankWndSchema,
  PutBackToBankSchema,
  PutGoldBackSchema,
  PutItemBackSchema,
} from '../../gen/proto/flyff/v1/bank_pb';

/** `NULL_ID` as the wire carries it: the banker NPC opened the session (no instant-bank item). */
export const BANK_NULL_ID = 0xffff_ffff;

export interface BankSenders {
  /** `SendOpenBankWnd(dwId, dwItemId)`: `(NULL_ID, 0)` at a banker, the scroll's object and item ids for the instant bank. */
  openBank(objId: number, itemId: number): void;
  /** `SendCloseBankWnd`. */
  closeBank(): void;
  /** `SendConfirmBank(pass, dwId, dwItemId)`. */
  confirmBank(pass: string, objId: number, itemId: number): void;
  /** `SendChangeBankPass(old, new, dwId, dwItemId)`. */
  changePass(oldPass: string, newPass: string, objId: number, itemId: number): void;
  /** `SendPutItemBank(slot, id, num)`: the bag item into compartment `slot`. */
  putItem(slot: number, objId: number, num: number): void;
  /** `SendGetItemBank(slot, id, num)`: the compartment item into the bag. */
  getItem(slot: number, objId: number, num: number): void;
  /** `SendPutGoldBank(slot, gold)`. */
  putGold(slot: number, gold: number): void;
  /** `SendGetGoldBank(slot, gold)`. */
  getGold(slot: number, gold: number): void;
  /** `SendPutItemBankToBank(putSlot, slot, id, num)`: compartment `src` to compartment `dst`. */
  moveItem(src: number, dst: number, objId: number, num: number): void;
  /** `SendPutGoldBankToBank(putSlot, slot, gold)`. */
  moveGold(src: number, dst: number, gold: number): void;
}

export function createBankSenders(ctx: ClientContext): BankSenders {
  const send = ctx.net.sender;
  return {
    openBank(objId, itemId) {
      // legacy-send: SendOpenBankWnd
      send.send({ case: 'openBankWnd', value: create(OpenBankWndSchema, { objId, itemId }) });
    },
    closeBank() {
      // legacy-send: SendCloseBankWnd
      send.send({ case: 'closeBankWnd', value: create(CloseBankWndSchema, {}) });
    },
    confirmBank(pass, objId, itemId) {
      // legacy-send: SendConfirmBank
      send.send({ case: 'confirmBank', value: create(ConfirmBankSchema, { pass, objId, itemId }) });
    },
    changePass(oldPass, newPass, objId, itemId) {
      // legacy-send: SendChangeBankPass
      send.send({
        case: 'changeBankPassReq',
        value: create(ChangeBankPassReqSchema, { oldPass, newPass, objId, itemId }),
      });
    },
    putItem(slot, objId, num) {
      // legacy-send: SendPutItemBank
      send.send({ case: 'putItemBack', value: create(PutItemBackSchema, { slot, objId, num }) });
    },
    getItem(slot, objId, num) {
      // legacy-send: SendGetItemBank
      send.send({ case: 'getItemBack', value: create(GetItemBackSchema, { slot, objId, num }) });
    },
    putGold(slot, gold) {
      // legacy-send: SendPutGoldBank
      send.send({ case: 'putGoldBack', value: create(PutGoldBackSchema, { slot, gold }) });
    },
    getGold(slot, gold) {
      // legacy-send: SendGetGoldBank
      send.send({ case: 'getGoldBack', value: create(GetGoldBackSchema, { slot, gold }) });
    },
    moveItem(src, dst, objId, num) {
      // legacy-send: SendPutItemBankToBank
      send.send({
        case: 'putBackToBank',
        value: create(PutBackToBankSchema, { flag: 1, srcSlot: src, dstSlot: dst, objId, num }),
      });
    },
    moveGold(src, dst, gold) {
      // legacy-send: SendPutGoldBankToBank
      send.send({
        case: 'putBackToBank',
        value: create(PutBackToBankSchema, { flag: 0, srcSlot: src, dstSlot: dst, gold }),
      });
    },
  };
}
