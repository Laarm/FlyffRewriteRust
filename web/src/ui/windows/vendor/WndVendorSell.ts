// `WndVendorSell` (`CWndRegVend`, `src:_Interface/WndRegVend.cpp`, ref/22 §17 F, ref/99 §4.7, Plan/42 §19.4 F, 45 §15.1; WP-UI-26):
// `APP_VENDOREX_SELL` (304 x 160), the registration of an item in the personal shop: the QUANTITY (`WIDC_SELLNUM`, starts at the stack) and the
// UNIT PRICE (`WIDC_SELLPRI`, starts at 0), both money statics typed on a number pad (`WIDC_NUM0`-`NUM9`, `00`, `000`, Min, Max and a
// four-function calculator `+ - * /` with `=`). A click on a field moves the focus marker (`WndVenderArrowEx.tga` at the field's top-left
// minus (4, 4)); the keyboard types digits (`OnChar`), Backspace deletes, Escape resets the focused number to 0. With a stack of one the price is
// focused from the start.
//
// OK parses the digits (quantity below 1 does nothing, above the stack is clamped, a price below 1 does nothing) and asks the request, which
// checks the total-gold rule and sends; the window closes when the request says so. Cancel closes it. While it is open Esc and the digit
// hot keys do nothing (the key contract, `WindowManager.isVendorWindowOpen`).
//
// The window ids of its buttons are the same as those of `APP_VENDOREX_BUY` (`ButItemNum*`).

import {
  APP_VENDOREX_SELL,
  WIDC_CANCEL,
  WIDC_NUM00,
  WIDC_NUM000,
  WIDC_OK,
  WIDC_SELLNUM,
  WIDC_SELLPRI,
  WIDC_VENDIVID,
  WIDC_VENEQUA,
  WIDC_VENMAX,
  WIDC_VENMIN,
  WIDC_VENMINUS,
  WIDC_VENMULTI,
  WIDC_VENPLUS,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import type { KeyInfo, WndBase } from '../../wnd/WndBase';
import { type Point, type Rect, rect } from '../../layout/geometry';
import { WSS_MONEY, Wnm } from '../../layout/styles';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  atoi,
  charOfKey,
  initialPad,
  INT_MAX,
  type PadState,
  padChar,
  padEquals,
  padMin,
  padZeros,
  SELL_LIMITS,
} from './keypad';
import { FOCUS_MARKER, FOCUS_MARKER_SIZE, PRICE_MAX } from './constants';
import { PAD_NUMBER_IDS } from './padIds';
import type { VendorSellRequest } from './ports';

/** The id of the code-created marker layer. */
export const WIDC_MARKER = 20001;
/** `m_dwFocus`: 0 the quantity, 1 the unit price. */
export type SellFocus = 0 | 1;

export class WndVendorSell extends WndNeuz {
  readonly appId = APP_VENDOREX_SELL;
  readonly #request: VendorSellRequest;
  #pads: [PadState, PadState];
  #focus: SellFocus;
  #num: Static | null = null;
  #price: Static | null = null;
  #marker: SpriteLayer | null = null;
  #numRect: Rect = rect(0, 0, 0, 0);
  #priceRect: Rect = rect(0, 0, 0, 0);

  constructor(request: VendorSellRequest) {
    super();
    this.#request = request;
    // `SetItem`: the quantity starts at the stack, the price at 0 (`__S8_VENDOR_REVISION`); a stack of one goes straight to the price
    this.#pads = [initialPad(String(request.stack)), initialPad('0')];
    this.#focus = request.stack <= 1 ? 1 : 0;
  }

  // ---- accessors for tests and the feature layer -----------------------------------------------------------------------

  get focus(): SellFocus {
    return this.#focus;
  }
  /** The quantity and the price as the statics show them (raw digits). */
  get texts(): { quantity: string; price: string } {
    return { quantity: this.#pads[0].text, price: this.#pads[1].text };
  }
  get quantityField(): Static | null {
    return this.#num;
  }
  get priceField(): Static | null {
    return this.#price;
  }
  get marker(): SpriteLayer | null {
    return this.#marker;
  }
  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }
  get request(): VendorSellRequest {
    return this.#request;
  }

  // ---- OnInitialUpdate -------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#num = this.getDlgItem<Static>(WIDC_SELLNUM);
    this.#price = this.getDlgItem<Static>(WIDC_SELLPRI);
    this.#num?.addWndStyle(WSS_MONEY);
    this.#price?.addWndStyle(WSS_MONEY);
    this.#numRect = this.getWndCtrl(WIDC_SELLNUM)?.rect ?? this.#numRect;
    this.#priceRect = this.getWndCtrl(WIDC_SELLPRI)?.rect ?? this.#priceRect;
    this.#marker = this.addControl(new SpriteLayer(), WIDC_MARKER, this.getClientRect(false));
    this.#show();
    this.setFocus();
    this.moveParentCenter();
  }

  /** Writes the two numbers and the marker. */
  #show(): void {
    const num = this.#pads[0].text;
    const price = this.#pads[1].text;
    if (this.#num && this.#num.getTitle() !== num) this.#num.setTitle(num);
    if (this.#price && this.#price.getTitle() !== price) this.#price.setTitle(price);
    const r = this.#focus === 0 ? this.#numRect : this.#priceRect;
    this.#marker?.setEntries([
      {
        key: 'focus',
        path: FOCUS_MARKER,
        x: r.l - 4,
        y: r.t - 4,
        w: FOCUS_MARKER_SIZE.w,
        h: FOCUS_MARKER_SIZE.h,
      },
    ]);
  }

  #pad(): PadState {
    return this.#pads[this.#focus];
  }
  #set(next: PadState): void {
    this.#pads[this.#focus] = next;
    this.#show();
  }

  // ---- input -------------------------------------------------------------------------------------------------------------------

  /** `OnLButtonUp`: a click inside a number field moves the focus to it. */
  override onLButtonUp(pt: Point): void {
    const inside = (r: Rect): boolean => pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b;
    if (inside(this.#numRect)) {
      this.#focus = 0;
      this.#show();
    } else if (inside(this.#priceRect)) {
      this.#focus = 1;
      this.#show();
    }
  }

  /** `OnChar` for the keys of the pad. */
  override onKeyDown(code: string, info: KeyInfo): void {
    const ch = charOfKey(code, info.shift);
    if (ch === null) return;
    this.#set(padChar(this.#pad(), ch, SELL_LIMITS, true));
  }

  /** A pad button or OK / Cancel (`OnChildNotify`). */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    this.setFocus(); // every notification gives the window the keyboard again
    if (msg !== Wnm.CLICKED) return super.onChildNotify(msg, id, src);
    const digit = PAD_NUMBER_IDS.get(id);
    if (digit !== undefined) {
      this.#set(padChar(this.#pad(), digit, SELL_LIMITS, true));
      return true;
    }
    switch (id) {
      case WIDC_VENPLUS:
        this.#set(padChar(this.#pad(), '+', SELL_LIMITS, true));
        return true;
      case WIDC_VENMINUS:
        this.#set(padChar(this.#pad(), '-', SELL_LIMITS, true));
        return true;
      case WIDC_VENMULTI:
        this.#set(padChar(this.#pad(), '*', SELL_LIMITS, true));
        return true;
      case WIDC_VENDIVID:
        this.#set(padChar(this.#pad(), '/', SELL_LIMITS, true));
        return true;
      case WIDC_VENEQUA:
        this.#set(padEquals(this.#pad()));
        return true;
      case WIDC_NUM00:
        this.#set(padZeros(this.#pad(), 2, SELL_LIMITS));
        return true;
      case WIDC_NUM000:
        this.#set(padZeros(this.#pad(), 3, SELL_LIMITS));
        return true;
      case WIDC_VENMIN:
        this.#set(padMin(this.#pad()));
        return true;
      case WIDC_VENMAX:
        // the quantity goes to the stack, the price to 2 100 000 000
        this.#set({
          ...this.#pad(),
          text: this.#focus === 0 ? String(this.#request.stack) : String(PRICE_MAX),
        });
        return true;
      case WIDC_OK:
        this.ok();
        return true;
      case WIDC_CANCEL:
        this.destroy();
        return true;
      default:
        return super.onChildNotify(msg, id, src);
    }
  }

  /**
   * `WIDC_OK`: the quantity (below 1: nothing happens, above the stack: the stack), the price as `int64` clamped to `INT_MAX` (below 1: nothing
   * happens); the request sends and the window closes when it says so.
   */
  ok(): boolean {
    const qty0 = atoi(this.#pads[0].text);
    if (qty0 < 1) return false;
    const qty = Math.min(qty0, this.#request.stack);
    const priceText = this.#pads[1].text;
    const price = Math.min(Number(priceText.replace(/[^0-9-]/g, '') || '0'), INT_MAX);
    if (!(price >= 1)) return false;
    const sent = this.#request.register(this.#request.slot, qty, priceText);
    if (sent) this.destroy();
    return sent;
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
