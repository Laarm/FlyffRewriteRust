// `WndPartyConfirm` (`CWndPartyConfirm`, `src:_Interface/WndPartyConfirm.cpp:1-121`, ref/22 §17 H, Plan/42 §19.4 H, 45 §22.1; WP-UI-39):
// `APP_PARTY_CONFIRM` (288 x 128), the invitation of a leader: "`<leader>` invites you to join the party. Do you accept?" (`TID_DIAG_0072`)
// in the read-only text control, Yes (`WIDC_YES`) and No (`WIDC_NO`). A normal window that opens centred horizontally 70 px from the top.
// Yes asks the caller to join (`SendAddPartyMember`); No and the close button decline (`SendPartyMemberCancle`). A window that the program
// closes (a duel begins: `SAFE_DELETE(m_pWndPartyConfirm)`) answers nothing.

import { APP_PARTY_CONFIRM, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PARTY_CONFIRM_Y } from './partyGeometry';
import type { PartyConfirmRequest } from './ports';

export { PARTY_CONFIRM_Y };

export class WndPartyConfirm extends WndNeuz {
  readonly appId = APP_PARTY_CONFIRM;
  readonly #request: PartyConfirmRequest;
  #answered = false;
  #text: TextCtrl | null = null;

  constructor(request: PartyConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }
  /** An answer was given (Yes, No or the close button). */
  get answered(): boolean {
    return this.#answered;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#request.text);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: PARTY_CONFIRM_Y,
    });
  }

  yes(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.yes();
    this.destroy();
  }

  no(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.no();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.yes();
        return true;
      }
      if (id === WIDC_NO || id === WTBID_CLOSE) {
        this.no();
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
