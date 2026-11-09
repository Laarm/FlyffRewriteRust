// `WndPartyLeaveConfirm` (`CWndPartyLeaveConfirm`, `src:_Interface/WndPartyChangeTroup.cpp:309-360`, ref/22 §17 H, Plan/42 §19.4 H,
// 45 §22.2; WP-UI-39): `APP_PARTYLEAVE_CONFIRM` (288 x 128), "Are you going to leave the party?" in a disabled edit control, Yes
// (`WIDC_YES`) and No (`WIDC_NO`). The window is centred. Yes asks the caller to send `RemovePartyMember`; No and the close button just
// close it.

import { APP_PARTYLEAVE_CONFIRM, WIDC_EDIT1, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PartyLeaveRequest } from './ports';

export class WndPartyLeaveConfirm extends WndNeuz {
  readonly appId = APP_PARTYLEAVE_CONFIRM;
  readonly #request: PartyLeaveRequest;
  #edit: EditCtrl | null = null;

  constructor(request: PartyLeaveRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.setString(this.#request.text);
    this.#edit?.enableWindow(false);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_NO || id === WTBID_CLOSE) {
        this.destroy();
        return true;
      }
      if (id === WIDC_YES) {
        this.#request.yes();
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
