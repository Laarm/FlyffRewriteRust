// `WndGuildPayConfirm` (`CWndGuildPayConfirm`, `src:_Interface/WndGuildTabApp.cpp:480-560`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-46): `APP_GUILD_PAYCONFIRM` (288 x 144), "What
// amount will you set for payment?" for ONE member level (the request carries the rank). A MODAL window with an edit control (`WIDC_EDIT1`) and OK (`WIDC_YES`) / Cancel (`WIDC_NO`).
// OK hands the text to the caller, which checks the digits and the range 0..999 999 and sends `GuildPenyaReq{rank, penya}`; a refusal opens `TID_GAME_GUILDONLYNUMBER`, the window
// clears the edit and stays. Cancel closes.

import { APP_GUILD_PAYCONFIRM, WIDC_EDIT1, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { PayRequest } from './ports';

export class WndGuildPayConfirm extends WndNeuz {
  readonly appId = APP_GUILD_PAYCONFIRM;
  readonly #request: PayRequest;
  #edit: EditCtrl | null = null;

  constructor(request: PayRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  get rank(): number {
    return this.#request.rank;
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
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        if (this.#request.submit(this.#edit?.getString() ?? '')) this.destroy();
        else this.#edit?.setString('');
        return true;
      }
      if (id === WIDC_NO) {
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
