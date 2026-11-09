// `WndSecretRoomInfoBox` (`CWndSecretRoomInfoMessageBox`, `src:_Interface/WndSecretRoom.cpp:1020-1145`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_MSG` (710, 320 x 176), the
// information box with a Yes / No pair. `SecretRoomMngState{4}` carries its text ("The Secret Room has opened. Would you like to teleport to the [Secret Room Manager] ...?"): YES asks the
// caller to teleport (`SendSecretRoomTeleportToNPC`; the server honours it inside the worlds 151 .. 167 only, so from Madrigal it does nothing visible) and closes; NO and the title
// close button (`WTBID_CLOSE`) just close (KEEP: no other way back).

import { APP_SECRETROOM_MSG, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { InfoBoxRequest } from './ports';

export class WndSecretRoomInfoBox extends WndNeuz {
  readonly appId = APP_SECRETROOM_MSG;
  readonly #request: InfoBoxRequest;

  constructor(request: InfoBoxRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.textCtrl?.setString(this.#request.text);
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
