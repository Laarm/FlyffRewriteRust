// `WndDropConfirm` (`CWndDropConfirm`, `src:_Interface/WndField.cpp:231-330`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4;
// WP-UI-22): `APP_DROP_CONFIRM` (272 x 160), "really drop this item?" for a single item: Yes (the default button) sends the
// drop, No closes. Like the quantity box it closes itself when the item becomes reserved.

import { APP_DROP_CONFIRM, WIDC_NO, WIDC_STATIC1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DropConfirmRequest } from './ports';

export class WndDropConfirm extends WndNeuz {
  readonly appId = APP_DROP_CONFIRM;
  readonly #request: DropConfirmRequest;

  constructor(request: DropConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<Button>(WIDC_YES)?.setDefault(true);
    if (this.#request.text) this.getDlgItem<Static>(WIDC_STATIC1)?.setTitle(this.#request.text);
    this.moveParentCenter();
  }

  override process(now: number): void {
    if (this.#request.reserved()) {
      this.destroy();
      return;
    }
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if ((msg === Wnm.CLICKED && id === WIDC_YES) || msg === Wnm.EN_RETURN) {
      this.#request.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_NO) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
