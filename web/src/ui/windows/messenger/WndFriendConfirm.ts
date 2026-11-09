// `WndFriendConfirm` (`CWndFriendConFirm`, `src:_Interface/WndFriendConFirm.cpp:1-110`, Plan/42 §19.4 I, 45 §24.1; WP-UI-43): `APP_FRIEND_CONFIRM` (288 x 128), the friend
// request of a player: "`<name>` wants to add you as a friend. Do you accept?" (`TID_DIAG_0071`) in the read-only text control, Yes (`WIDC_YES`) and No (`WIDC_NO`).
// A normal window that opens centred horizontally 70 px from the top. Yes asks the request to accept, No to decline; the close button and a window the program closes
// answer nothing (the original only handled the two buttons).

import { APP_FRIEND_CONFIRM, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { FriendConfirmRequest } from './ports';

/** `CPoint point( ( rectRoot.right - rectWindow.Width() ) / 2, 70 )`. */
export const FRIEND_CONFIRM_Y = 70;

export class WndFriendConfirm extends WndNeuz {
  readonly appId = APP_FRIEND_CONFIRM;
  readonly #request: FriendConfirmRequest;
  #text: TextCtrl | null = null;
  #answered = false;

  constructor(request: FriendConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }
  /** Yes or No was clicked. */
  get answered(): boolean {
    return this.#answered;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#request.text);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: FRIEND_CONFIRM_Y,
    });
  }

  yes(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.yes();
    this.destroy();
  }

  no(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.no();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.yes();
        return true;
      }
      if (id === WIDC_NO) {
        this.no();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
