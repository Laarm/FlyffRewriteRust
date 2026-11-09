// `WndConfirmBank` (`CWndConfirmBank`, `src:_Interface/WndBank.cpp:432-520`, ref/22 §17 E, Plan/42 §19.4 E, 45 §13.1; WP-UI-25): `APP_CONFIRM_BANK`
// (272 x 160), the PIN entry of the bank. One masked field for the four digits (the field takes digits only and at most four, so the refusal
// of the original - `TID_DIAG_0025` "Password should have 4 digits" - can only come from a short PIN, which empties the field); OK or Enter
// sends the PIN, "Change PIN" asks the feature for the PIN window and closes this one, Cancel closes it. The window opens in the middle of the
// screen (`Move(right - width, 110); MoveParentCenter()`).

import {
  APP_CONFIRM_BANK,
  WIDC_CANCEL,
  WIDC_CHANGE_PASS,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, EBS_PASSWORD, Wnm } from '../../layout/styles';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { checkConfirmPin } from './pin';
import { PIN_LENGTH, type ConfirmBankRequest } from './ports';

export interface PinWindowDeps {
  readonly msgbox: Pick<MessageBoxApi, 'open'>;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
}

export class WndConfirmBank extends WndNeuz {
  readonly appId = APP_CONFIRM_BANK;
  readonly #deps: PinWindowDeps;
  readonly #request: ConfirmBankRequest;
  #edit: EditCtrl | null = null;
  #replaced = false;

  constructor(deps: PinWindowDeps, request: ConfirmBankRequest) {
    super();
    this.#deps = deps;
    this.#request = request;
    this.putRegInfo = false;
  }

  /** The PIN field (`WIDC_EDIT1`). */
  get edit(): EditCtrl | null {
    return this.#edit;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const edit = this.getDlgItem<EditCtrl>('WIDC_EDIT1');
    if (edit) {
      // `AddWndStyle(EBS_PASSWORD)`; FIX: the field also takes digits only (WP-UI-25) and at most four of them
      edit.addWndStyle(EBS_PASSWORD | EBS_NUMBER);
      edit.setMaxLength(PIN_LENGTH);
      edit.setFocus();
    }
    this.#edit = edit;
    this.moveParentCenter();
  }

  /** The next window of the flow replaces this one: `closed` reports `replaced` (the session must stay). */
  markReplaced(): void {
    this.#replaced = true;
  }

  /** OK or Enter. Returns whether the PIN went out. */
  ok(): boolean {
    const edit = this.#edit;
    if (!edit) return false;
    const text = edit.getString();
    const tid = checkConfirmPin(text);
    if (tid !== 0) {
      edit.setString('');
      this.#deps.msgbox.open(this.#deps.getText(tid));
      return false;
    }
    this.#request.confirm(text);
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_RETURN || (msg === Wnm.CLICKED && id === WIDC_OK)) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CHANGE_PASS) {
      this.#replaced = true;
      this.#request.changePassword();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.(this.#replaced ? 'replaced' : 'user');
    super.onDestroy();
  }
}
