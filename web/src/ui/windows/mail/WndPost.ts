// `WndPost` (`CWndPost`, `src:_Interface/WndField.cpp:14333-14440`, ref/22 §17 I, Plan/42 §19.4 I, 45 §16; WP-UI-27): `APP_POST` (320 x 368), the post window. One
// tab control with two pages, "Send Mail" (`APP_POST_SEND`) and "Inbox" (`APP_POST_RECEIVE`, the one that is selected). The window opens DISABLED, asks for the
// mailbox (`queryBox`) and puts the modal requesting box over itself; the answer (`mailboxArrived`) enables the window and closes the box. Choosing the inbox
// tab asks for the mailbox again. The window centres itself.

import { APP_POST, APP_REQUESTING_POST, WIDC_TABCTRL1 } from '../../../gen/defs/resdata';
import { TID_APP_POST_RECEIVE, TID_APP_POST_SEND } from '../../../gen/defs/text';
import type { TabCtrl } from '../../controls/TabCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { TAB_RECEIVE, TAB_SEND } from './names';
import type { PostRequest } from './ports';
import { WndPostReceive } from './WndPostReceive';
import { WndPostSend } from './WndPostSend';

export interface PostDeps {
  readonly request: PostRequest;
  readonly msgbox: Pick<MessageBoxApi, 'open' | 'openUpper'>;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  /** Opens the requesting box over the window (`new CWndMailRequestingBox; Initialize()`). */
  openRequesting(): void;
}

export class WndPost extends WndNeuz {
  readonly appId = APP_POST;
  readonly #deps: PostDeps;
  #tab: TabCtrl | null = null;
  #send: WndPostSend | null = null;
  #receive: WndPostReceive | null = null;

  constructor(deps: PostDeps) {
    super();
    this.#deps = deps;
  }

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get sendPage(): WndPostSend | null {
    return this.#send;
  }
  get receivePage(): WndPostReceive | null {
    return this.#receive;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `EnableWindow( FALSE )` until the mailbox is here
    this.enableWindow(false);
    const tab = this.getDlgItem<TabCtrl>(WIDC_TABCTRL1);
    this.#tab = tab;
    if (tab) {
      const send = new WndPostSend({
        ports: this.#deps.request.send,
        msgbox: this.#deps.msgbox,
        tooltip: this.#deps.tooltip,
        getText: this.#deps.getText,
      });
      send.attach(tab);
      const receive = new WndPostReceive({
        view: this.#deps.request.view,
        actions: this.#deps.request.actions,
        tooltip: this.#deps.tooltip,
      });
      receive.attach(tab);
      this.#send = send;
      this.#receive = receive;
      tab.insertItem(TAB_SEND, { text: this.#deps.getText(TID_APP_POST_SEND), page: send });
      tab.insertItem(TAB_RECEIVE, {
        text: this.#deps.getText(TID_APP_POST_RECEIVE),
        page: receive,
      });
      tab.setCurSel(TAB_RECEIVE);
    }
    this.moveParentCenter();
    // the list of the mails that arrived, and the box that waits for it
    this.#deps.request.actions.queryBox();
    this.#deps.openRequesting();
  }

  /** `OnMailBox`: the window is enabled, the list is refreshed (the pages follow the view), the requesting box closes. */
  mailboxArrived(): void {
    this.enableWindow(true);
    this.env.wm.get(APP_REQUESTING_POST)?.destroy();
  }

  /** The reply of the reader: the send tab gets the sender and the title and comes to the front. */
  showSend(fill: { receiver: string; title: string; text: string }): void {
    this.#send?.fill(fill);
    this.#tab?.setCurSel(TAB_SEND);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.SELCHANGE && id === this.#tab?.id) {
      // the inbox tab asks for the mailbox again; the send tab does nothing
      if (this.#tab.getCurSel() !== TAB_SEND) this.#deps.request.actions.queryBox();
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    // `CloseMailRequestingBox`
    this.env.wm.get(APP_REQUESTING_POST)?.destroy();
    this.#deps.request.actions.closed();
    super.onDestroy();
  }
}
