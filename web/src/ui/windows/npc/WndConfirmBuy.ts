// `WndConfirmBuy` (`CWndConfirmBuy`, `src:_Interface/WndShop.cpp:290-560`, ref/22 §17 E, Plan/45 §12.2; WP-UI-24):
// `APP_CONFIRM_BUYEX` (272 x 176), the quantity box of a purchase: a quantity edit (starts at 1), `+` / `-` / `Max` buttons
// and the price of the quantity (`WIDC_STATIC2`, thousands separators). The price formula is NOT here: the request computes
// it (the wasm `buyUnitPrice` behind it, Plan/42 §4.5 rule 1). The ceiling of the quantity is the request's (99; 9999 for
// arrows, charms and Red Chips; 21 for the Perin). Enter = OK.

import {
  APP_CONFIRM_BUYEX,
  WIDC_CANCEL,
  WIDC_CONTROL2,
  WIDC_MAX,
  WIDC_MINUS,
  WIDC_OK,
  WIDC_PLUS,
  WIDC_STATIC2,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { ConfirmBuyRequest } from './ports';
import { INT_MAX, atoi } from './quantity';

export { INT_MAX, atoi };

export class WndConfirmBuy extends WndNeuz {
  readonly appId = APP_CONFIRM_BUYEX;
  readonly #request: ConfirmBuyRequest;
  #edit: EditCtrl | null = null;
  #total: Static | null = null;
  #ok: Button | null = null;

  constructor(request: ConfirmBuyRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get quantityEdit(): EditCtrl | null {
    return this.#edit;
  }
  get totalText(): Static | null {
    return this.#total;
  }
  get okButton(): Button | null {
    return this.#ok;
  }
  get plusButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_PLUS);
  }
  get minusButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_MINUS);
  }
  get maxButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_MAX);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  /** The quantity in the edit (`atoi(m_pEdit->GetString())`). */
  get quantity(): number {
    return atoi(this.#edit?.getString() ?? '');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_CONTROL2);
    this.#total = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#ok = this.getDlgItem<Button>(WIDC_OK);
    this.#total?.addWndStyle(WSS_MONEY);
    this.#edit?.addWndStyle(EBS_NUMBER);
    this.#ok?.setDefault(true);
    this.#edit?.setFocus();
    this.moveParentCenter();
    this.#change(1);
  }

  /** `OnChangeBuyCount`: the edit shows `n`; the price follows, and a total beyond `INT_MAX` takes one piece back. */
  #change(n: number): void {
    let count = n;
    let total = this.#request.total(count);
    if (total > INT_MAX && count > 1) {
      count -= 1;
      total = this.#request.total(count);
    }
    const edit = this.#edit;
    if (edit && atoi(edit.getString()) !== count) edit.setString(String(count));
    this.#total?.setTitle(String(total));
  }

  #max(): number {
    return this.#request.maxQuantity;
  }

  /** OK: the request checks and sends; the window closes (`OnOK(); Destroy();`). */
  ok(): void {
    this.#request.confirm(this.quantity);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_RETURN) {
      this.ok();
      return true;
    }
    if (msg === Wnm.EN_CHANGE && id === WIDC_CONTROL2) {
      this.#change(Math.min(Math.max(this.quantity, 0), this.#max()));
      return true;
    }
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_PLUS) {
        this.#change(Math.min(this.quantity + 1, this.#max()));
        return true;
      }
      if (id === WIDC_MINUS) {
        this.#change(Math.max(this.quantity - 1, 1));
        return true;
      }
      if (id === WIDC_MAX) {
        this.#change(this.#max());
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
      if (id === WIDC_OK) {
        this.ok();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
