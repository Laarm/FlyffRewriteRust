// `WndGuildWarJoinConfirm` (`CWndGuildWarJoinConfirm( nCombatType )`, `src:_Interface/WndField.cpp:17476-17590`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.4; WP-UI-49): the entrance question of the siege on
// the layout of `APP_CLOSE_EXISTING_CONNECTION` (266, 256 x 144, Yes / No only; the text is a text control made in code at `(8, 8, w - 4, h / 2)`). Mode 0 asks `TID_GAME_GUILDWAR_JOIN` in the
// colour of its id, mode 1 `TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE_MSG`. YES: a vagrant gets the box "a vagrant cannot enter" and nothing is sent, anyone else asks the caller to enter (mode 0
// `SendGCJoin`, mode 1 `SendGC1to1TeleportToStage`); both buttons close. The title is `TID_GAME_GUILDCOMBAT_ENTRANCE` / `_1TO1_ENTRANCE`.
// The legacy `CWndGuildWarAppConfirm` (a fixed 80 000 penya application on this layout) has no opener and is not ported.

import { APP_CLOSE_EXISTING_CONNECTION, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE,
  TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE_MSG,
  TID_GAME_GUILDCOMBAT_ENTRANCE,
  TID_GAME_GUILDCOMBAT_NOTENTER_VAGRANT,
  TID_GAME_GUILDWAR_JOIN,
} from '../../../gen/defs/text';
import { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NODRAWFRAME, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { JoinRequest, SiegeHost } from './ports';
import { DEFAULT_TEXT_COLOR } from './WndGuildWarCancelConfirm';

export interface JoinDeps {
  readonly host: SiegeHost;
}

export class WndGuildWarJoinConfirm extends WndNeuz {
  readonly appId = APP_CLOSE_EXISTING_CONNECTION;
  readonly #request: JoinRequest;
  readonly #host: SiegeHost;
  #text: TextCtrl | null = null;

  constructor(request: JoinRequest, deps: JoinDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get mode(): 0 | 1 {
    return this.#request.mode;
  }
  get textCtrl(): TextCtrl | null {
    return this.#text;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    this.setTitle(
      this.#host.text(
        r.mode === 0 ? TID_GAME_GUILDCOMBAT_ENTRANCE : TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE,
      ),
    );
    // `rect = GetClientRect(); rect.left += 8; rect.right -= 4; rect.top += 8; rect.bottom /= 2`
    const client = this.rectClient;
    const text = new TextCtrl();
    this.#text = this.addControl(
      text,
      0,
      {
        l: 8,
        t: 8,
        r: rectWidth(client) - 4,
        b: Math.trunc(rectHeight(client) / 2),
      },
      WBS_NODRAWFRAME,
    );
    text.setString(
      this.#host.text(
        r.mode === 0 ? TID_GAME_GUILDWAR_JOIN : TID_GAME_GUILDCOMBAT_1TO1_ENTRANCE_MSG,
      ),
      r.textColor ?? DEFAULT_TEXT_COLOR,
    );
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        if (this.#request.vagrant()) {
          this.#host.box(this.#host.text(TID_GAME_GUILDCOMBAT_NOTENTER_VAGRANT));
        } else {
          this.#request.join();
        }
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
