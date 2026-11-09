// `WndSecretRoomCancelConfirm` (`CWndSecretRoomCancelConfirm`, `src:_Interface/WndSecretRoom.cpp:1147-1210`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_CANCEL_CONFIRM` (760,
// 336 x 160), "20% of money will be charged if you cancel. Do you want to cancel the secret room?" (`TID_GAME_SECRETROOM_CANCEL`, drawn in BLACK `0xff000000`) with Yes / No. YES asks the caller
// to cancel the bid (`SendSecretRoomTenderCancelReturn`), both buttons close. The title is the layout's.

import {
  APP_SECRETROOM_CANCEL_CONFIRM,
  WIDC_NO,
  WIDC_TEXT1,
  WIDC_YES,
} from '../../../gen/defs/resdata';
import { TID_GAME_SECRETROOM_CANCEL } from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { CancelRequest, SecretRoomHost } from './ports';

export interface CancelDeps {
  readonly host: SecretRoomHost;
}

/** The colour of the text (`0xff000000`, what the original passed). */
export const CANCEL_TEXT_COLOR = 0xff000000;

export class WndSecretRoomCancelConfirm extends WndNeuz {
  readonly appId = APP_SECRETROOM_CANCEL_CONFIRM;
  readonly #request: CancelRequest;
  readonly #host: SecretRoomHost;

  constructor(request: CancelRequest, deps: CancelDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.textCtrl?.setString(this.#host.text(TID_GAME_SECRETROOM_CANCEL), CANCEL_TEXT_COLOR);
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
