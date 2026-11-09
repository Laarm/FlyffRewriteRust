// `WndBankPassword` (`CWndBankPassword`, `src:_Interface/WndBank.cpp:522-700`, ref/22 §17 E, Plan/42 §19.4 E, 45 §13.1; WP-UI-25):
// `APP_BANK_PASSWORD` (272 x 208), the first PIN and the PIN change. Three masked fields (the old PIN, the new PIN, the confirmation) that
// take digits only and at most four of them. In the first-PIN mode (`m_nFlags == 0`) the old field holds "0000" and the focus starts in the
// new field (a click on the old one moves it there). OK or Enter checks (`checkChangePin`: the order of the refusals of the original, each
// with its message box and the fields it empties) and sends `old` and `new`; Cancel closes.

import { WIDC_CANCEL, WIDC_OK, APP_BANK_PASSWORD } from '../../../gen/defs/resdata';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, EBS_PASSWORD, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DEFAULT_PIN } from './names';
import { checkChangePin } from './pin';
import { PIN_LENGTH, type BankPasswordRequest } from './ports';
import type { PinWindowDeps } from './WndConfirmBank';

export class WndBankPassword extends WndNeuz {
  readonly appId = APP_BANK_PASSWORD;
  readonly #deps: PinWindowDeps;
  readonly #request: BankPasswordRequest;
  #old: EditCtrl | null = null;
  #new: EditCtrl | null = null;
  #confirm: EditCtrl | null = null;
  #replaced = false;

  constructor(deps: PinWindowDeps, request: BankPasswordRequest) {
    super();
    this.#deps = deps;
    this.#request = request;
    this.putRegInfo = false;
  }

  /** The three fields: the old PIN, the new PIN and the confirmation. */
  get fields(): {
    readonly old: EditCtrl | null;
    readonly next: EditCtrl | null;
    readonly confirm: EditCtrl | null;
  } {
    return { old: this.#old, next: this.#new, confirm: this.#confirm };
  }

  get initial(): boolean {
    return this.#request.initial;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const old = this.getDlgItem<EditCtrl>('WIDC_EDIT1');
    const next = this.getDlgItem<EditCtrl>('WIDC_EDIT2');
    const confirm = this.getDlgItem<EditCtrl>('WIDC_EDIT3');
    this.#old = old;
    this.#new = next;
    this.#confirm = confirm;
    if (old && next && confirm) {
      for (const e of [old, next, confirm]) {
        e.addWndStyle(EBS_PASSWORD | EBS_NUMBER);
        e.setMaxLength(PIN_LENGTH);
      }
      if (this.#request.initial) {
        old.setString(DEFAULT_PIN);
        next.setFocus();
      } else {
        old.setFocus();
      }
    }
    this.moveParentCenter();
  }

  /** The next window of the flow replaces this one: `closed` reports `replaced`. */
  markReplaced(): void {
    this.#replaced = true;
  }

  #resetAll(resetOld: boolean): void {
    if (resetOld) this.#old?.setString(this.#request.initial ? DEFAULT_PIN : '');
    this.#new?.setString('');
    this.#confirm?.setString('');
  }

  /** OK or Enter. Returns whether the request went out. */
  ok(): boolean {
    const old = this.#old;
    const next = this.#new;
    const confirm = this.#confirm;
    if (!old || !next || !confirm) return false;
    const check = checkChangePin(old.getString(), next.getString(), confirm.getString());
    if (!check.ok) {
      this.#resetAll(check.resetOld);
      this.#deps.msgbox.open(this.#deps.getText(check.tid));
      return false;
    }
    this.#request.change(old.getString(), next.getString());
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_RETURN || (msg === Wnm.CLICKED && id === WIDC_OK)) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    // `m_nFlags == 0 && message == 0 && nID == WIDC_EDIT1`: the old PIN is "0000", the focus goes to the new one
    if (this.#request.initial && msg === Wnm.CLICKED && id === this.#old?.id) {
      this.#new?.setFocus();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.(this.#replaced ? 'replaced' : 'user');
    super.onDestroy();
  }
}
