// `WndGuildCombatInfoBox` (`CGuildCombatInfoMessageBox( nCombatType )`, `src:_Interface/WndField.cpp:16236-16360`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-49): `APP_GUILDCOMBAT_MSG` (563,
// 320 x 176), the information box with a leave button. `GC_TELE` carries its text ("... do you want to go back to Madrigal?"): YES asks the caller to teleport (`SendGCTele`, mode 1
// `SendGC1to1TeleportToNPC`) and closes, NO and the close button just close. The title follows the original's `PaintFrame`: `TID_GAME_GUILDCOMBAT_OFFERSTATE` / `_1TO1_OFFERSTATE` (a copy of the
// offer-state window's title; KEEP).

import { APP_GUILDCOMBAT_MSG, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT_1TO1_OFFERSTATE,
  TID_GAME_GUILDCOMBAT_OFFERSTATE,
} from '../../../gen/defs/text';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { InfoBoxRequest, SiegeHost } from './ports';

export interface InfoBoxDeps {
  readonly host: SiegeHost;
}

export class WndGuildCombatInfoBox extends WndNeuz {
  readonly appId = APP_GUILDCOMBAT_MSG;
  readonly #request: InfoBoxRequest;
  readonly #host: SiegeHost;

  constructor(request: InfoBoxRequest, deps: InfoBoxDeps) {
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
          ? TID_GAME_GUILDCOMBAT_OFFERSTATE
          : TID_GAME_GUILDCOMBAT_1TO1_OFFERSTATE,
      ),
    );
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
