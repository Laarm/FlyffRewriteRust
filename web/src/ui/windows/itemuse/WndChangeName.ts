// `WndChangeName` (`CWndChangeName` / `CWndChangePetName`, `src:_Interface/WndChangeName.cpp`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.6;
// WP-UI-34 / WP-CFEAT-15): `APP_CHANGENAME` (224 x 128), "Please input the new name to change into." with one edit, OK and Cancel. OK takes the
// trimmed text and asks the request to `validate` it (`IsValidPlayerName`: empty, length, first character a digit, letters; then `IsInvalidName`
// / `IsAllowedLetter` -> `TID_DIAG_0020`): a refusal opens a message box and keeps the focus in the edit, a good name goes to `confirm` (the pet
// naming scroll sends `DoUseItemInput`, the character rename scroll `QuerySetPlayerName`) and the window closes. Cancel and the close button
// close it (the forced rename of a server merge that cannot be cancelled is M6).

import {
  APP_CHANGENAME,
  WIDC_CANCEL,
  WIDC_EDIT_CHANGENAME,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { ChangeNameRequest } from './ports';

export class WndChangeName extends WndNeuz {
  readonly appId = APP_CHANGENAME;
  readonly #request: ChangeNameRequest;
  #edit: EditCtrl | null = null;

  constructor(request: ChangeNameRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT_CHANGENAME);
    this.#edit?.setFocus();
    this.moveParentCenter();
  }

  /** OK: validates the trimmed name; a refusal stays open with the edit focused. Returns whether the name was accepted. */
  ok(): boolean {
    const name = (this.#edit?.getString() ?? '').trim();
    const tid = this.#request.validate(name);
    if (tid > 0) {
      this.#request.error(tid);
      this.#edit?.setFocus();
      return false;
    }
    this.#request.confirm(name);
    this.destroy();
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
