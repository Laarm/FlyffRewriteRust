// `WndDuelConfirm` (`CWndDuelConfirm`, `src:_Interface/WndDuelConfirm.cpp:1-455`, ref/22 §17 K, Plan/42 §19.4 K, 45 §25.1; WP-UI-41):
// `APP_DUEL_CONFIRM` (288 x 128). The question ("`<name>` challenges you to a duel. Do you accept?", the party text for a party duel) sits
// in the layout's text control, Yes (`WIDC_YES`) and No (`WIDC_NO`) below. The window opens centred horizontally, 70 px from the top
// (over the gauges). The plan makes it modal with Enter = Yes (the original is a normal window: KEEP-NOTE in `NOTES.md`); the title
// bar's close button is No (`nID == WTBID_CLOSE`). What Yes sends is the caller's (`closeTransactional('DuelAccepted')` then `DuelYes`
// for a solo duel, `DuelPartyYes` without the clean-up for a party duel).

import { APP_DUEL_CONFIRM, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_KEY, WBS_MODAL, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DUEL_WINDOW_Y } from './geometry';
import type { DuelConfirmRequest } from './ports';

export { DUEL_WINDOW_Y };

export class WndDuelConfirm extends WndNeuz {
  readonly appId = APP_DUEL_CONFIRM;
  readonly #request: DuelConfirmRequest;
  #answered = false;
  #text: TextCtrl | null = null;

  constructor(request: DuelConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_DUEL_CONFIRM)`; modal with the keyboard on the frame (Enter presses the default Yes button). */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
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
  /** An answer was given (Yes, No or the close button). */
  get answered(): boolean {
    return this.#answered;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#request.text);
    this.#text?.enableWindow(false);
    this.yesButton?.setDefault(true);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: DUEL_WINDOW_Y,
    });
  }

  /** Yes / Enter: the request's `yes()`, then the window closes (the answer is given once). */
  yes(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.yes();
    this.destroy();
  }

  /** No / the close button: the request's `no()`, then the window closes. */
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
      if (id === WIDC_NO || id === WTBID_CLOSE) {
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
