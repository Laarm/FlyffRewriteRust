// `WndSecretRoomBoard` (`CWndSecretRoomBoard`, `src:_Interface/WndSecretRoom.cpp:1213-1285`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_BOARD` (747, 384 x 384), the read-only
// information board of the secret-room manager: the panel `secretroomboard` of the lazy help bundle in `WIDC_TEXT1`, `BUTTON1` closes. The title is the layout's. A missing panel gives an
// empty text.

import { APP_SECRETROOM_BOARD, WIDC_BUTTON1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { BoardRequest } from './ports';

export class WndSecretRoomBoard extends WndNeuz {
  readonly appId = APP_SECRETROOM_BOARD;
  readonly #request: BoardRequest;

  constructor(request: BoardRequest) {
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
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
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
