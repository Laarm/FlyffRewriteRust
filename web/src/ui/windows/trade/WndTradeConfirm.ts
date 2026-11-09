// `WndTradeConfirm` (`CWndTradeConfirm`, `src:_Interface/WndField.cpp:9235-9302`, ref/22 §17 F, Plan/42 §19.4 F, 45 §14.2; WP-UI-26):
// `APP_TRADE_CONFIRM` (288 x 128), the last question of a trade: both players pressed OK, "Are you sure you want to trade?" (`TID_DIAG_0083`)
// with Yes (`SendTradeConfirm`) and No / the close button (`SendTradeCancel`). It opens centred on the root. After the own answer arrives
// (`TradeLastConfirmOk` of the own player) `wait()` swaps the text for "Please wait ..." (`TID_GAME_WAITCOMFIRM`), hides both buttons and
// makes the window deaf (`bMsg`); only the trade's end closes it.

import { APP_TRADE_CONFIRM, WIDC_NO, WIDC_STATIC1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { TradeFinalConfirm } from './ports';

export class WndTradeConfirm extends WndNeuz {
  readonly appId = APP_TRADE_CONFIRM;
  readonly #request: TradeFinalConfirm;
  #waiting = false;
  #text: Static | null = null;

  constructor(request: TradeFinalConfirm) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): Static | null {
    return this.#text;
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }
  /** `bMsg`: the own confirmation was accepted by the server. */
  get waiting(): boolean {
    return this.#waiting;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.yesButton?.setVisible(true);
    this.noButton?.setVisible(true);
    this.#text = this.getDlgItem<Static>(WIDC_STATIC1);
    this.#text?.setTitle(this.#request.question);
    this.moveParentCenter();
  }

  /** `OnTradelastConfirmOk` (own id): `bMsg = TRUE`, the wait text, both buttons hidden. */
  wait(text: string): void {
    this.#waiting = true;
    this.#text?.setTitle(text);
    this.yesButton?.setVisible(false);
    this.noButton?.setVisible(false);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (this.#waiting) return true; // `if( bMsg ) return( TRUE )`: every notification is swallowed, the close button too
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_NO) {
        this.#request.no();
        return true;
      }
      if (id === WTBID_CLOSE) {
        // the title-bar button also falls through to `CWndNeuz::OnChildNotify`, which destroys the window
        this.#request.no();
        this.destroy();
        return true;
      }
      if (id === WIDC_YES) {
        this.#request.yes();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
