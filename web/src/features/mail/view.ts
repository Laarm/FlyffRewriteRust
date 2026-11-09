// The read model of the post window (`MailView` of `ui/windows/mail/ports.ts`, Plan/45 §16; WP-CFEAT-23): the store's box turned into rows with the sender's
// name, the time that is left, its colour, the date of arrival and the custody fee. The view keeps its identity while nothing it shows changed; the time
// left moves with the minute.

import {
  TID_GAME_MAIL_RECEIVE_DATE,
  TID_PK_LIMIT_DAY,
  TID_PK_LIMIT_HOUR,
} from '../../gen/defs/text';
import { II_SYS_SYS_SCR_SEALCHARACTER } from '../../gen/defs/item';
import type { ClientDataIndex } from '../../data';
import type { MailRow, MailView } from '../../ui/windows/mail/ports';
import { ItemViewCache } from '../common/itemView';
import { formatTemplate } from '../common/text';
import type { InvItem } from '../inventory/items';
import { inCustody, keepColor, keepInfo, keepText } from './keep';
import type { MailEntry, MailState } from './state';

export interface MailViewEnv {
  data(): Pick<ClientDataIndex, 'item' | 'version'> | null | undefined;
  text(tid: number): string;
  /** Unix seconds now. */
  nowSec(): number;
  /** The name of a character, `''` while it is not known (the cache queries it). */
  senderName(id: number): string;
  /** `rules.mailStorageFee(GetCost(item), elapsed)`. */
  fee(item: InvItem, elapsedSec: number): number;
  friends(): readonly string[];
  newFontColor(): boolean;
  /** The date of arrival as the locale writes it (`COleDateTime::Format`). */
  formatDate(sec: number): string;
}

interface Cached {
  readonly key: string;
  readonly row: MailRow;
}

export class MailViewBuilder {
  readonly #items: ItemViewCache;
  #rows = new WeakMap<MailEntry, Cached>();
  #last: MailView | null = null;
  #lastKey = '';
  #friends: readonly string[] = [];

  constructor(private readonly env: MailViewEnv) {
    this.#items = new ItemViewCache(() => env.data());
  }

  #row(e: MailEntry, nowSec: number): MailRow {
    const minute = Math.floor(nowSec / 60);
    const name = e.senderId === 0 ? 'FLYFF' : this.env.senderName(e.senderId);
    const key = `${String(minute)}|${name}|${String(this.env.data()?.version ?? -1)}`;
    const hit = this.#rows.get(e);
    if (hit?.key === key) return hit.row;
    const info = keepInfo(e.createdAt, nowSec);
    const item = e.item;
    const custody = item !== null && inCustody(info);
    const row: MailRow = {
      id: e.id,
      sender: name === '' ? 'Unknown' : name,
      senderKnown: name !== '',
      title: e.title,
      text: e.text,
      gold: e.gold,
      item: e.item ? this.#items.view(e.item) : null,
      read: e.read,
      deletable: e.item?.itemId !== II_SYS_SYS_SCR_SEALCHARACTER,
      keepText: keepText(info, this.env.text(TID_PK_LIMIT_HOUR), this.env.text(TID_PK_LIMIT_DAY)),
      keepColor: keepColor(info.days),
      keepOver: info.days <= 0,
      receivedText: formatTemplate(this.env.text(TID_GAME_MAIL_RECEIVE_DATE), [
        this.env.formatDate(e.createdAt),
      ]),
      custody,
      fee: item !== null && custody ? this.env.fee(item, Math.max(0, nowSec - e.createdAt)) : 0,
    };
    this.#rows.set(e, { key, row });
    return row;
  }

  build(state: MailState): MailView {
    const now = this.env.nowSec();
    const friends = this.env.friends();
    if (friends.length !== this.#friends.length || friends.some((f, i) => f !== this.#friends[i])) {
      this.#friends = friends;
    }
    const rows = state.box.map((e) => this.#row(e, now));
    const key = [
      state.version,
      this.env.data()?.version ?? -1,
      this.env.newFontColor(),
      rows.map((r) => r.sender + r.keepText).join('\u0001'),
      this.#friends.join('\u0001'),
    ].join('|');
    if (this.#last && key === this.#lastKey) return this.#last;
    const view: MailView = {
      version: state.version,
      rows,
      newFontColor: this.env.newFontColor(),
      friends: this.#friends,
    };
    this.#last = view;
    this.#lastKey = key;
    return view;
  }
}
