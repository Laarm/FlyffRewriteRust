// `WndConfirmSell` (`CWndConfirmSell`, `src:_Interface/WndShop.cpp:118-290`, ref/22 §17 E, Plan/45 §12.3; WP-UI-24):
// `APP_CONFIRM_SELL` (272 x 176, `WBS_KEY`), "really sell this item?": a quantity edit that starts at the whole stack
// (1 for a single item and then fixed at 1), clamped to the stack, and the price of the quantity (`WIDC_STATIC2`). OK (the
// default button, Enter) sells, Cancel closes. The window closes itself when the item becomes reserved
// (`__DROP_CONFIRM_BUG`) or is gone. The price comes from the request (the wasm `sellUnitPrice` behind it).

import {
  APP_CONFIRM_SELL,
  WIDC_CANCEL,
  WIDC_EDITSELL,
  WIDC_OK,
  WIDC_STATIC2,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, WBS_KEY, WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { atoi } from './quantity';
import type { ConfirmSellRequest } from './ports';

export class WndConfirmSell extends WndNeuz {
  readonly appId = APP_CONFIRM_SELL;
  readonly #request: ConfirmSellRequest;
  #edit: EditCtrl | null = null;
  #total: Static | null = null;
  #ok: Button | null = null;
  #shownTotal = '';

  constructor(request: ConfirmSellRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_CONFIRM_SELL, WBS_KEY)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | extraStyle);
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
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDITSELL);
    this.#total = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#ok = this.getDlgItem<Button>(WIDC_OK);
    this.#total?.addWndStyle(WSS_MONEY);
    this.#edit?.addWndStyle(EBS_NUMBER);
    this.#ok?.setDefault(true);
    this.#edit?.setFocus();
    this.moveParentCenter();
    // the whole stack to start with (`m_nItemNum`), "1" for a single item
    this.#edit?.setString(String(Math.max(1, this.#request.count())));
    this.#refresh();
  }

  /** The pieces that OK would sell (the edit, clamped to the stack). */
  get quantity(): number {
    return atoi(this.#edit?.getString() ?? '');
  }

  /** `OnDraw`: a single item always reads 1, a larger number than the stack is cut to it; the price follows. */
  #refresh(): void {
    const edit = this.#edit;
    const count = this.#request.count();
    if (edit && this.#request.gone()) {
      this.#setTotal('0');
      return;
    }
    if (edit) {
      if (count === 1) {
        if (edit.getString() !== '1') edit.setString('1');
      } else if (this.quantity > count) {
        edit.setString(String(count));
      }
    }
    this.#setTotal(String(this.#request.total(Math.min(this.quantity, count))));
  }

  #setTotal(text: string): void {
    if (text === this.#shownTotal) return;
    this.#shownTotal = text;
    this.#total?.setTitle(text);
  }

  override process(now: number): void {
    // `__DROP_CONFIRM_BUG`: an item that became reserved (a trade, the quick sell) cannot be sold any more
    if (this.#request.reserved()) {
      this.destroy();
      return;
    }
    this.#refresh();
    super.process(now);
  }

  /** OK or Enter: the sale of a non-zero quantity, then the window closes. */
  ok(): void {
    const n = Math.min(this.quantity, this.#request.count());
    if (this.#request.count() >= 1 && n !== 0) this.#request.confirm(n);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_CHANGE && id === WIDC_EDITSELL) {
      this.#refresh();
      return true;
    }
    if (msg === Wnm.EN_RETURN || (msg === Wnm.CLICKED && id === WIDC_OK)) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
