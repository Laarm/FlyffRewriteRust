// `WndPartyChangeName` (`CWndPartyChangeName`, `src:_Interface/WndPartyChangeTroup.cpp:25-215`, ref/22 §17 H, Plan/42 §19.4 H, 45 §22.2;
// WP-UI-40): `APP_PARTYCHANGENAME` (272 x 160), "Enter Party Name" with an edit control, OK (`WIDC_OK`) and Cancel (`WIDC_CANCEL`). The edit
// starts with the last name the character used (else the default party name). OK hands the trimmed text to the caller, which runs the checks
// of the original (length, first character, special characters, forbidden names: a refused name opens a message box and the window stays) and
// sends the name; the window closes when the caller accepted it. Cancel closes. The window is centred.

import { APP_PARTYCHANGENAME, WIDC_CANCEL, WIDC_EDIT1, WIDC_OK } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PartyChangeNameRequest } from './ports';

export class WndPartyChangeName extends WndNeuz {
  readonly appId = APP_PARTYCHANGENAME;
  readonly #request: PartyChangeNameRequest;
  #edit: EditCtrl | null = null;

  constructor(request: PartyChangeNameRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.setString(this.#request.initial);
    this.moveParentCenter();
  }

  /** OK: the caller checks and sends; the window closes when the name was taken. */
  submit(): boolean {
    const name = (this.#edit?.getString() ?? '').trim();
    if (!this.#request.submit(name)) return false;
    this.destroy();
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.submit();
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
