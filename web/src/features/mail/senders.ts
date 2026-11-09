// The sends of the mail windows (rule C9; `// legacy-send:` markers name the legacy send function, ref/99 App. B).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  QueryGetMailGoldSchema,
  QueryGetMailItemSchema,
  QueryMailBoxReqSchema,
  QueryPostMailSchema,
  QueryRemoveMailSchema,
  ReadMailSchema,
} from '../../gen/proto/flyff/v1/mail_pb';

/** `0xFF`: no item attached (`QueryPostMail.obj_id`, kept verbatim). */
export const NO_ATTACHMENT = 0xff;

export interface MailSenders {
  /** `SendQueryMailBox`. */
  queryBox(): void;
  /** `SendQueryReadMail(nMail)`. */
  readMail(id: number): void;
  /** `SendQueryGetMailItem(nMail)`. */
  getItem(id: number): void;
  /** `SendQueryGetMailGold(nMail)`. */
  getGold(id: number): void;
  /** `SendQueryRemoveMail(nMail)`. */
  removeMail(id: number): void;
  /** `SendQueryPostMail(item, num, receiver, gold, title, text)`; `objId = NO_ATTACHMENT` and `num = 0` without an item. */
  postMail(m: {
    objId: number;
    num: number;
    receiver: string;
    gold: number;
    title: string;
    text: string;
  }): void;
}

export function createMailSenders(ctx: ClientContext): MailSenders {
  const send = ctx.net.sender;
  return {
    queryBox() {
      // legacy-send: SendQueryMailBox
      send.send({ case: 'queryMailBoxReq', value: create(QueryMailBoxReqSchema, {}) });
    },
    readMail(mailId) {
      // legacy-send: SendQueryReadMail
      send.send({ case: 'readMail', value: create(ReadMailSchema, { mailId }) });
    },
    getItem(mailId) {
      // legacy-send: SendQueryGetMailItem
      send.send({ case: 'queryGetMailItem', value: create(QueryGetMailItemSchema, { mailId }) });
    },
    getGold(mailId) {
      // legacy-send: SendQueryGetMailGold
      send.send({ case: 'queryGetMailGold', value: create(QueryGetMailGoldSchema, { mailId }) });
    },
    removeMail(mailId) {
      // legacy-send: SendQueryRemoveMail
      send.send({ case: 'queryRemoveMail', value: create(QueryRemoveMailSchema, { mailId }) });
    },
    postMail(m) {
      // legacy-send: SendQueryPostMail
      send.send({ case: 'queryPostMail', value: create(QueryPostMailSchema, m) });
    },
  };
}
