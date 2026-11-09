// `WndPartyChangeTroup` (`CWndPartyChangeTroup`, `src:_Interface/WndPartyChangeTroup.cpp:216-307`, ref/22 §17 H, Plan/42 §19.4 H,
// 45 §22.2; WP-UI-40): `APP_CHANGETROUP` (272 x 128), "Convert to an Advanced Party?" with OK (`WIDC_OK`) and Cancel (`WIDC_CANCEL`). OK
// closes the window and asks the caller for the name window; Cancel just closes it. The window is centred.

import { APP_CHANGETROUP, WIDC_CANCEL, WIDC_OK } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PartyChangeTroupRequest } from './ports';

export class WndPartyChangeTroup extends WndNeuz {
  readonly appId = APP_CHANGETROUP;
  readonly #request: PartyChangeTroupRequest;

  constructor(request: PartyChangeTroupRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.#request.ok();
        this.destroy();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
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
