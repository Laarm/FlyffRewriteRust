// `WndGuildWarCancelConfirm` (`CWndGuildWarCancelConfirm( nCombatType )`, `src:_Interface/WndField.cpp:17367-17475`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-50): `APP_GUILDCOMBAT_CANCEL_CONFIRM`
// (561, 336 x 160), "Do you want to withdraw the application?" (`TID_GAME_GUILDWAR_CANCEL`, drawn in the colour of its text id) with Yes / No. YES asks the caller to cancel the bid (mode 0:
// `SendGuildCombatCancel`, mode 1: `SendGC1to1TenderCancel`), both buttons close. The title is that of the mode (`PaintFrame`): `TID_GAME_GUILDCOMBAT_CANCEL_CONFIRM` / `_1TO1_CANCEL_CONFIRM`. The
// original created a text control of its own over the layout's `WIDC_TEXT1`; the layout's control is used.

import {
  APP_GUILDCOMBAT_CANCEL_CONFIRM,
  WIDC_NO,
  WIDC_TEXT1,
  WIDC_YES,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT_1TO1_CANCEL_CONFIRM,
  TID_GAME_GUILDCOMBAT_CANCEL_CONFIRM,
  TID_GAME_GUILDWAR_CANCEL,
} from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { CancelRequest, SiegeHost } from './ports';

export interface CancelDeps {
  readonly host: SiegeHost;
}

/** The colour of a text whose id has none (`0xff000000`, what the original passed). */
export const DEFAULT_TEXT_COLOR = 0xff000000;

export class WndGuildWarCancelConfirm extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_CANCEL_CONFIRM;
  readonly #request: CancelRequest;
  readonly #host: SiegeHost;

  constructor(request: CancelRequest, deps: CancelDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get mode(): 0 | 1 {
    return this.#request.mode;
  }
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.setTitle(
      this.#host.text(
        this.#request.mode === 0
          ? TID_GAME_GUILDCOMBAT_CANCEL_CONFIRM
          : TID_GAME_GUILDCOMBAT_1TO1_CANCEL_CONFIRM,
      ),
    );
    this.textCtrl?.setString(
      this.#host.text(TID_GAME_GUILDWAR_CANCEL),
      this.#request.textColor ?? DEFAULT_TEXT_COLOR,
    );
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.#request.yes();
        this.destroy();
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
