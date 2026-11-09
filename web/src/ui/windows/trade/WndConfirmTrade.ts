// `WndConfirmTrade` (`CWndConfirmTrade`, `src:_Interface/WndField.cpp:9158-9232`, ref/22 §17 F, Plan/42 §19.4 F, 45 §14.1; WP-UI-26):
// `APP_CONFIRM_TRADE` (272 x 160), the request of a player who wants to trade: "<name>" in the first static (`TID_GAME_FROM`) over "Would
// you like to trade?". OK accepts (`SendTrade`), Cancel and the close button decline (`SendConfirmTradeCancel`); both close the window. A
// window that the program closes (a duel begins, the requester left) answers nothing. It opens centred horizontally 70 px from the top.

import { APP_CONFIRM_TRADE, WIDC_CANCEL, WIDC_OK, WIDC_STATIC2 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CONFIRM_TRADE_Y } from './constants';
import type { TradeRequestWindow } from './ports';

export class WndConfirmTrade extends WndNeuz {
  readonly appId = APP_CONFIRM_TRADE;
  readonly #request: TradeRequestWindow;
  #answered = false;
  #from: Static | null = null;

  constructor(request: TradeRequestWindow) {
    super();
    this.#request = request;
  }

  get fromText(): Static | null {
    return this.#from;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }
  get requesterName(): string {
    return this.#request.name;
  }
  /** An answer was given (OK, Cancel or the close button). */
  get answered(): boolean {
    return this.#answered;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#from = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#from?.setTitle(this.#request.fromText);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: CONFIRM_TRADE_Y,
    });
  }

  /** OK: `SendTrade( pTrader )`, then `Destroy()`. */
  accept(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.accept();
    this.destroy();
  }

  /** Cancel / close: `SendConfirmTradeCancel( m_objid )`, then `Destroy()`. */
  decline(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.decline();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.accept();
        return true;
      }
      if (id === WIDC_CANCEL || id === WTBID_CLOSE) {
        this.decline();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
