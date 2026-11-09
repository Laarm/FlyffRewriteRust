// `WndLordConfirm` (`CWndLordConfirm`, `src:_Interface/WndLord.cpp:905-970`, ref/22 §17 O, Plan/42 §19.4 O; WP-UI-55): `APP_LORD_CONFIRM` (738, 336 x 160, "OK"), the confirmation of a changed pledge
// (`TID_GAME_MOD_PLEGDE`: it costs 1 000 000 penya). `BUTTON1` with a non-empty pledge sends it, hides the window and closes the pledge window; `BUTTON2` only hides the window. (45 §28.2 lists a
// "confirm Yes" row for the vote; the C++ wins: 738 is the re-pledge confirmation, the vote has no confirm window, its answer is the `TID_GAME_VOTE_COMPLETE` box.)

import {
  APP_LORD_CONFIRM,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_GAME_MOD_PLEGDE } from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordConfirmRequest, LordWindowDeps, LordWindowOps } from './ports';

export class WndLordConfirm extends WndNeuz {
  readonly appId = APP_LORD_CONFIRM;
  readonly #request: LordConfirmRequest;
  readonly #deps: LordWindowDeps;
  readonly #ops: LordWindowOps;

  constructor(request: LordConfirmRequest, deps: LordWindowDeps, ops: LordWindowOps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.#ops = ops;
    this.putRegInfo = false;
  }

  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get pledge(): string {
    return this.#request.pledge;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.textCtrl?.setString(this.#deps.host.text(TID_GAME_MOD_PLEGDE));
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        if (this.#request.pledge !== '') {
          this.#deps.ports.actions.setPledge(this.#request.pledge);
          // `SetVisible( FALSE )` and the pledge window goes (its destructor takes this window with it)
          this.setVisible(false);
          this.#request.sent();
          this.#ops.closePledge();
        }
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.setVisible(false);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
