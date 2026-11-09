// `WndQuitRoom` (`CWndQuitRoom`, `src:_Interface/WndQuitRoom.cpp:75-89`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): `APP_QUIT_ROOM` (304 x 128), "Are you sure you want to exit?" at the door of the mini room (`MMI_RETURNTO_WORLD`).
// `BUTTON1` asks the server to leave (`SendHousingGoOut`) and closes, `BUTTON2` closes.

import { APP_QUIT_ROOM, WIDC_BUTTON1, WIDC_BUTTON2 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QuitRoomRequest } from './ports';

export class WndQuitRoom extends WndNeuz {
  readonly appId = APP_QUIT_ROOM;
  readonly #request: QuitRoomRequest;

  constructor(request: QuitRoomRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.#request.goOut();
        this.destroy();
        return true;
      }
      if (id === WIDC_BUTTON2) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
