// `WndLordRPInfo` (`CWndLordRPInfo`, `src:_Interface/WndLord.cpp:1150-1198`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-55): `APP_LORD_INFO2` (758, 400 x 304, "Bid money return info Window"), the table
// of the returned bids. `TEXT1` shows the panel `lordrpinfo` of the help bundle, `BUTTON1` closes. No shipped NPC lists the menu entry 102 (`MaFl_KINGAIDE` has it commented out), so the window is reachable
// through the `npcMenuRun` probe hook only.

import { APP_LORD_INFO2, WIDC_BUTTON1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordWindowDeps } from './ports';

export class WndLordRPInfo extends WndNeuz {
  readonly appId = APP_LORD_INFO2;
  readonly #deps: LordWindowDeps;

  constructor(deps: LordWindowDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get infoText(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const text = this.#deps.ports.panel('lordrpinfo');
    if (text !== '') this.infoText?.setString(text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
