// `MailWindowsService` (`ui.mail`, Plan/42 §19.4 I; WP-UI-27): the post window and its helper windows.
//
//   ui.mail.openPost(request)             APP_POST: the send tab and the inbox over the requesting box
//   ui.mail.mailboxArrived()              the box is here: the post window is enabled and the requesting box closes
//   ui.mail.openRead(request)             APP_POST_READ: one mail (modal)
//   ui.mail.openDeleteConfirm(request)    APP_POST_DELETE_CONFIRM (modal)
//   ui.mail.openRequesting(request)       APP_REQUESTING_POST (modal)
//   ui.mail.closeAll()                    every mail window goes (a transactional close)
//
// Each `open*` replaces the window of the same kind. The requests carry the views and the callbacks; what they send is 45's (`features/mail`).

import {
  APP_POST,
  APP_POST_DELETE_CONFIRM,
  APP_POST_READ,
  APP_REQUESTING_POST,
} from '../../../gen/defs/resdata';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { MAIL_LAZY } from './lazy';
import type { DeleteConfirmRequest, PostRequest, ReadRequest, RequestingBoxRequest } from './ports';
import type { WndMailRequesting } from './WndMailRequesting';
import type { WndPost } from './WndPost';
import type { WndPostDeleteConfirm } from './WndPostDeleteConfirm';
import type { WndPostRead } from './WndPostRead';

export * from './ports';
export { byteLength, checkSendForm, cutText, goldOf, type SendCheck } from './form';
// the shells load on demand (`lazy.ts`): only their types are exported here, the window ids and timings live in `names.ts`
export type { WndMailRequesting, WndPost, WndPostDeleteConfirm, WndPostRead };
export type { WndPostReceive } from './WndPostReceive';
export type { WndPostSend } from './WndPostSend';
export {
  DESTRUCTION_PRESSES,
  REQUEST_WAIT_MS,
  TAB_RECEIVE,
  TAB_SEND,
  WIDC_MAIL_LIST,
  WIDC_READ_COIN,
  WIDC_READ_ITEM,
  WIDC_SEND_ITEM,
  WIDC_SEND_RECEIVER,
} from './names';

export interface MailWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly msgbox: Pick<MessageBoxApi, 'open' | 'openUpper'>;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
}

export class MailWindowsService {
  readonly #wm: WindowManager;
  readonly #deps: MailWindowsDeps;

  constructor(deps: MailWindowsDeps) {
    this.#wm = deps.wm;
    this.#deps = deps;
  }

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  /** Opens the window of a lazy class (`null` while its chunk is still loading: the window then opens by itself). */
  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  /** `SAFE_DELETE( m_pWndPost ); m_pWndPost = new CWndPost; Initialize()`: the box that waits for the mailbox opens with it. */
  openPost(request: PostRequest): WndPost | null {
    return this.#open(
      APP_POST,
      MAIL_LAZY.WndPost,
      (C) =>
        new C({
          request,
          msgbox: this.#deps.msgbox,
          tooltip: this.#deps.tooltip,
          getText: this.#deps.getText,
          openRequesting: () => {
            this.openRequesting({
              request: () => {
                request.actions.queryBox();
              },
            });
          },
        }),
    );
  }

  openRead(request: ReadRequest): WndPostRead | null {
    return this.#open(
      APP_POST_READ,
      MAIL_LAZY.WndPostRead,
      (C) =>
        new C({
          request,
          msgbox: this.#deps.msgbox,
          tooltip: this.#deps.tooltip,
          getText: this.#deps.getText,
        }),
    );
  }

  openDeleteConfirm(request: DeleteConfirmRequest): WndPostDeleteConfirm | null {
    return this.#open(
      APP_POST_DELETE_CONFIRM,
      MAIL_LAZY.WndPostDeleteConfirm,
      (C) => new C({ getText: this.#deps.getText }, request),
    );
  }

  openRequesting(request: RequestingBoxRequest): WndMailRequesting | null {
    return this.#open(APP_REQUESTING_POST, MAIL_LAZY.WndMailRequesting, (C) => new C(request));
  }

  /** The post window, when it is open. */
  get post(): WndPost | null {
    return (this.#wm.get(APP_POST) as WndPost | null) ?? null;
  }
  get reader(): WndPostRead | null {
    return (this.#wm.get(APP_POST_READ) as WndPostRead | null) ?? null;
  }

  /** `OnMailBox`: enables the post window and closes the requesting box. */
  mailboxArrived(): void {
    const post = this.post;
    if (post) post.mailboxArrived();
    else this.#wm.get(APP_REQUESTING_POST)?.destroy();
  }

  /** Every mail window goes (the post window closes its requesting box itself). */
  closeAll(): void {
    for (const app of [APP_POST_DELETE_CONFIRM, APP_POST_READ, APP_REQUESTING_POST, APP_POST]) {
      this.#wm.get(app)?.destroy();
    }
    this.#wm.flushDestroyed();
  }
}
