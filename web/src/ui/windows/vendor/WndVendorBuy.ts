// `WndVendorBuy` (`CWndVendorBuy`, `src:_Interface/WndVendorBuy.cpp`, ref/22 §17 F, Plan/42 §19.4 F, 45 §15.2; WP-UI-26):
// `APP_VENDOREX_BUY` (304 x 160), the purchase from somebody else's shop: the QUANTITY (`WIDC_BUYNUM`, starts at everything that is for
// sale), the unit price (`WIDC_STATIC4`) and the total (`WIDC_STATIC5`), all money statics; the number pad of the sell window without the
// calculator. The quantity never passes the stock (`GetExtra()`): a larger number is replaced by the stock at once, and the total follows every
// press. OK needs a quantity of at least 1; the request checks the penya (a box over the window) and sends; the window closes when it says so.
//
// FIX: the total is computed exactly (the original multiplied two `int` and could print a wrapped negative number).

import {
  APP_VENDOREX_BUY,
  WIDC_BUYNUM,
  WIDC_CANCEL,
  WIDC_NUM00,
  WIDC_NUM000,
  WIDC_OK,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_VENMAX,
  WIDC_VENMIN,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { WSS_MONEY, Wnm } from '../../layout/styles';
import type { KeyInfo, WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  atoi,
  BUY_LIMITS,
  charOfKey,
  initialPad,
  type PadState,
  padChar,
  padMin,
  padZeros,
} from './keypad';
import { PAD_NUMBER_IDS } from './padIds';
import type { VendorBuyRequest } from './ports';

export class WndVendorBuy extends WndNeuz {
  readonly appId = APP_VENDOREX_BUY;
  readonly #request: VendorBuyRequest;
  #pad: PadState;
  #quantity: Static | null = null;
  #price: Static | null = null;
  #total: Static | null = null;

  constructor(request: VendorBuyRequest) {
    super();
    this.#request = request;
    this.#pad = initialPad(String(request.count));
  }

  get quantityField(): Static | null {
    return this.#quantity;
  }
  get priceField(): Static | null {
    return this.#price;
  }
  get totalField(): Static | null {
    return this.#total;
  }
  /** The quantity as typed (raw digits). */
  get quantityText(): string {
    return this.#pad.text;
  }
  /** The total price shown (`m_nCost * quantity`). */
  get total(): number {
    return this.#request.price * Math.min(atoi(this.#pad.text), this.#request.count);
  }
  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }
  get request(): VendorBuyRequest {
    return this.#request;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#quantity = this.getDlgItem<Static>(WIDC_BUYNUM);
    this.#price = this.getDlgItem<Static>(WIDC_STATIC4);
    this.#total = this.getDlgItem<Static>(WIDC_STATIC5);
    for (const s of [this.#quantity, this.#price, this.#total]) s?.addWndStyle(WSS_MONEY);
    this.#price?.setTitle(String(this.#request.price));
    this.#show();
    this.setFocus();
    this.moveParentCenter();
  }

  /** Clamps the quantity to the stock, writes the quantity and the total (the tail of `OnChar` / `OnChildNotify`). */
  #show(): void {
    let n = atoi(this.#pad.text);
    if (n > this.#request.count) {
      this.#pad = { ...this.#pad, text: String(this.#request.count) };
      n = this.#request.count;
    }
    const q = this.#pad.text;
    if (this.#quantity && this.#quantity.getTitle() !== q) this.#quantity.setTitle(q);
    this.#total?.setTitle(String(this.#request.price * n));
  }

  #apply(next: PadState): void {
    this.#pad = next;
    this.#show();
  }

  /** `OnChar` for the keys of the pad (no calculator here). */
  override onKeyDown(code: string, info: KeyInfo): void {
    const ch = charOfKey(code, info.shift);
    if (ch === null) return;
    this.#apply(padChar(this.#pad, ch, BUY_LIMITS, false));
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    this.setFocus();
    if (msg !== Wnm.CLICKED) return super.onChildNotify(msg, id, src);
    const digit = PAD_NUMBER_IDS.get(id);
    if (digit !== undefined) {
      this.#apply(padChar(this.#pad, digit, BUY_LIMITS, false));
      return true;
    }
    switch (id) {
      case WIDC_NUM00:
        this.#apply(padZeros(this.#pad, 2, BUY_LIMITS));
        return true;
      case WIDC_NUM000:
        this.#apply(padZeros(this.#pad, 3, BUY_LIMITS));
        return true;
      case WIDC_VENMIN:
        this.#apply(padMin(this.#pad));
        return true;
      case WIDC_VENMAX:
        this.#apply({ ...this.#pad, text: String(this.#request.count) });
        return true;
      case WIDC_OK:
        this.ok();
        return true;
      case WIDC_CANCEL:
        this.destroy();
        return true;
      default:
        // the originals' tail runs for every notification: the total is refreshed
        this.#show();
        return super.onChildNotify(msg, id, src);
    }
  }

  /** `WIDC_OK`: a quantity of at least 1; the request checks and sends, the window closes when it says so. */
  ok(): boolean {
    const n = atoi(this.#pad.text);
    if (n < 1) return false;
    const sent = this.#request.confirm(this.#request.slot, n);
    if (sent) this.destroy();
    return sent;
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
