// Ports of the mail windows (Plan/42 §19.4 I, WP-UI-27; wired by the `mail` feature of 45, WP-CFEAT-23): the post window with its two tabs (`APP_POST`,
// `APP_POST_SEND`, `APP_POST_RECEIVE`), the mail reader (`APP_POST_READ`), the delete confirmation (`APP_POST_DELETE_CONFIRM`) and the box that
// waits for the mailbox (`APP_REQUESTING_POST`). The READ MODELS (`*View`, observed through a `ViewSource`) and the ACTIONS the shells call; plain TS
// interfaces without protobuf types. Every action names the legacy send behind it (ref/99 App. B); the decisions (the throttle, the gates, the sends,
// the fee) are the feature's.

import type { Shortcut } from '../../dnd/shortcut';
import type { InventoryItem } from '../char/ports';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** `MAX_MAILTITLE` / `MAX_MAILTEXT` (`post.h`): the title holds at most 31 bytes, the text 255. */
export const MAX_MAILTITLE = 32;
export const MAX_MAILTEXT = 256;
/** `MAX_MAIL_LIST_PER_PAGE` (`WndField.cpp:73`): six rows of 40 px in the inbox. */
export const MAIL_ROWS_PER_PAGE = 6;
/** The most penya a mail carries (`liGold > 1000000000` -> `TID_GAME_LIMITPENYA`). */
export const MAX_MAIL_GOLD = 1_000_000_000;
/** `MAX_KEEP_MAX_DAY` / `MAX_KEEP_BASIC_DAY` (`post.h`, the shipped build). */
export const MAX_KEEP_MAX_DAY = 15;
export const MAX_KEEP_BASIC_DAY = 7;
/** The postage text of the layout shows 500 penya; the number the feature charges comes from the server. */

/** One mail of the inbox, as the list and the reader show it. */
export interface MailRow {
  /** `m_nMail`: what the sends carry. */
  readonly id: number;
  /** The sender's name: `FLYFF` for the system (id 0), the cached character name, `Unknown` while it is not known. */
  readonly sender: string;
  /** `m_idSender != 0` and the name is not known: a reply is refused. */
  readonly senderKnown: boolean;
  readonly title: string;
  readonly text: string;
  /** `m_nGold`. */
  readonly gold: number;
  /** The attached item (`m_pItemElem`) for the icon and the tooltip. */
  readonly item: InventoryItem | null;
  /** `m_byRead`. */
  readonly read: boolean;
  /** The mail may be deleted (not while it carries the scroll of a sealed character). */
  readonly deletable: boolean;
  /** The keep time as the list shows it: `TID_PK_LIMIT_HOUR` or `TID_PK_LIMIT_DAY` filled in. */
  readonly keepText: string;
  /** `0xAARRGGBB`: the keep time fades from red to dark green as the days run out (`D3DXVec2Lerp`). */
  readonly keepColor: number;
  /** `nDay <= 0`: the text sits 3 px further left. */
  readonly keepOver: boolean;
  /** `TID_GAME_MAIL_RECEIVE_DATE` filled with the date (the row tooltip). */
  readonly receivedText: string;
  /** `true` once the free keep time (7 days) is over: taking the item costs the custody fee (`MailReceiveItem`). */
  readonly custody: boolean;
  /** The custody fee of the item in penya (`rules.mailStorageFee`), 0 when none is due. */
  readonly fee: number;
}

export interface MailView {
  /** Bumps with every change of the box. */
  readonly version: number;
  /** The mails in box order. */
  readonly rows: readonly MailRow[];
  /** `features.new_font_color`. */
  readonly newFontColor: boolean;
  /** The names for the receiver drop-down of the send tab (`m_Messenger.m_aFriend`); empty until the messenger exists. */
  readonly friends: readonly string[];
}

/** The mail being written (the send tab). */
export interface MailDraft {
  /** The attached item (the inventory's record; `count` is the amount that is attached) or `null`. */
  readonly item: InventoryItem | null;
  /** The attached amount (`GetExtra()` of the item). */
  readonly count: number;
}

export interface MailSendForm {
  readonly receiver: string;
  readonly title: string;
  readonly text: string;
  /** The penya field as typed (digits only or empty). */
  readonly gold: string;
}

export interface PostActions {
  /**
   * The post window opened or the inbox tab was chosen: `SendQueryMailBox` (`legacy-send: SendQueryMailBox`). The feature sends at most one
   * request per 3 s (`MAILBOX_QUERY_MIN_INTERVAL_MS`) and the answer closes the requesting box.
   */
  queryBox(): void;
  /** Double click on a row: opens `APP_POST_READ` for the mail and tells the server it was read (`SendQueryReadMail`). */
  openMail(id: number): void;
  /** The window was destroyed. */
  closed(): void;
}

/** What the send tab needs of the world. */
export interface SendPorts {
  readonly view: ViewSource<MailView>;
  /** The item attached to the mail being written, with its reserved amount. */
  readonly draft: ViewSource<MailDraft>;
  /**
   * An inventory item dropped on the item slot (`CWndPostSend::OnDropIcon`): a worn item says `TID_GAME_EQUIPPUT`, a premium one
   * `TID_GAME_CHARGED_NOTUSE`, one the server would refuse `TID_GAME_CANNOT_POST`; a stack asks for the amount. Returns `{ name }` when the
   * item was taken (the original puts its name into the title field), `null` when it was refused.
   */
  attach(sc: Shortcut): { readonly name: string } | null;
  /** Right click on the slot (`OnRButtonUp`): the item leaves the mail. */
  detach(): void;
  /** `SendQueryPostMail(item, extra, receiver, gold, title, text)` after the player confirmed: the draft's item is cleared (`ClearData`). */
  send(form: MailSendForm): void;
}

export interface ReadActions {
  /** `MailReceiveItem`: the item goes to the bag (a custody fee asks first: `APP_QUEITMWARNING`) -> `SendQueryGetMailItem`. */
  takeItem(id: number): void;
  /** `MailReceiveGold` -> `SendQueryGetMailGold`. */
  takeGold(id: number): void;
  /** Delete -> `APP_POST_DELETE_CONFIRM` -> `SendQueryRemoveMail`. */
  confirmDelete(id: number): void;
  /** Reply: the send tab gets the sender and "Re:" + the title and takes the front. */
  reply(id: number): void;
  /** The reader was destroyed. */
  closed(): void;
}

export interface PostRequest {
  readonly actions: PostActions;
  readonly view: ViewSource<MailView>;
  readonly send: SendPorts;
}

export interface ReadRequest {
  /** The mail by id from the live box (`null` once it is gone: the reader closes). */
  readonly mail: () => MailRow | null;
  readonly view: ViewSource<MailView>;
  readonly actions: ReadActions;
}

export interface DeleteConfirmRequest {
  /** Yes -> `SendQueryRemoveMail`. */
  yes(): void;
}

export interface RequestingBoxRequest {
  /** The request button: `SendQueryMailBox` again. */
  request(): void;
}
