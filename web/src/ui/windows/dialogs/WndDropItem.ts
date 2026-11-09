// `WndDropItem` (`CWndDropItem`, `src:_Interface/WndField.cpp:89-230`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4; WP-UI-22):
// `APP_DROP_ITEM` (272 x 160), "drop how many?": a static question, a quantity edit that starts at the whole stack and is
// clamped to it, OK (the default button; Enter in the edit also confirms) and Cancel. A quantity of 0 sends nothing. The
// window closes itself when the item becomes reserved (`__DROP_CONFIRM_BUG`).

import {
  APP_DROP_ITEM,
  WIDC_CANCEL,
  WIDC_EDIT1,
  WIDC_OK,
  WIDC_STATIC1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DropItemRequest } from './ports';

export class WndDropItem extends WndNeuz {
  readonly appId = APP_DROP_ITEM;
  readonly #request: DropItemRequest;
  #edit: EditCtrl | null = null;
  #ok: Button | null = null;

  constructor(request: DropItemRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get quantityEdit(): EditCtrl | null {
    return this.#edit;
  }
  get okButton(): Button | null {
    return this.#ok;
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#ok = this.getDlgItem<Button>(WIDC_OK);
    this.#ok?.setDefault(true);
    // KEEP-QUIRK: digits only is enforced here by the edit style (the original rewrote the text with the whole stack
    // when a non-digit was typed); the clamp to the stack size is the same
    this.#edit?.addWndStyle(EBS_NUMBER);
    this.#edit?.setFocus();
    this.#edit?.setString(String(this.#request.count));
    if (this.#request.text) this.getDlgItem<Static>(WIDC_STATIC1)?.setTitle(this.#request.text);
    this.moveParentCenter();
  }

  /** `OnDraw`: a single item always reads 1; a larger number than the stack is cut to the stack. */
  #clamp(): void {
    const edit = this.#edit;
    if (!edit) return;
    const count = this.#request.count;
    if (count === 1) {
      if (edit.getString() !== '1') edit.setString('1');
      return;
    }
    const n = Number.parseInt(edit.getString(), 10);
    if (Number.isFinite(n) && n > count) edit.setString(String(count));
  }

  override process(now: number): void {
    // `__DROP_CONFIRM_BUG`: an item that became reserved cannot be dropped any more
    if (this.#request.reserved()) {
      this.destroy();
      return;
    }
    this.#clamp();
    super.process(now);
  }

  /** The quantity that OK would send (0 = nothing). */
  get quantity(): number {
    if (this.#request.count < 1) return 0;
    const n = Number.parseInt(this.#edit?.getString() ?? '', 10);
    return Number.isFinite(n) ? n : 0;
  }

  /** OK or Enter: `SendDropItem` for a non-zero quantity, then the window closes. */
  ok(): void {
    this.#clamp();
    const n = this.quantity;
    if (n !== 0) this.#request.confirm(n);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if ((msg === Wnm.CLICKED && id === WIDC_OK) || msg === Wnm.EN_RETURN) {
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
