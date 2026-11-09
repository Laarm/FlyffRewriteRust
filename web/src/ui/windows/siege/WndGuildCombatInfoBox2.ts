// `WndGuildCombatInfoBox2` (`CGuildCombatInfoMessageBox2`, `src:_Interface/WndField.cpp:16361-16410`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-50): `APP_GUILDCOMBAT_MSG2` (565, 320 x 176), the
// plain information box of `GC_DIAGMESSAGE` (the invitation to the siege, the entry refusals): the text of the server and OK. It keeps the title of its layout.

import { APP_GUILDCOMBAT_MSG2, WIDC_OK, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { InfoBox2Request } from './ports';

export class WndGuildCombatInfoBox2 extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_MSG2;
  readonly #request: InfoBox2Request;

  constructor(request: InfoBox2Request) {
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
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
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
