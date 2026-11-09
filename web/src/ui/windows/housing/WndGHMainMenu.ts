// `WndGHMainMenu` (`CWndGHMainMenu`, `src:_Interface/WndGuildHouse.cpp:16-228`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): `APP_GH_MAIN` (288 x 112), the menu of the guild house. Opened when the world becomes 212-214 and closed when it is
// left (the feature watches the world); NO close button (`m_bNoCloseButton`: the door NPC's menu 150 leaves), exempt from Esc (`ESC_KEEP_APPS` of the window manager), hidden and restored by numpad `*` with the other windows.
//
//   * `BTN_SETUPPOWER` toggles the guild window on its authority tab (4), `BTN_FURNITURE` / `BTN_TELEPORTER` open the storage window in that section, switch an open one and close it when it already shows that section, `BTN_STORAGE`
//     toggles the guild bank, `BTN_UPKEEP` checks the power (`TID_GAME_GUILDHOUSE_TAX_LEVEL`) and toggles the upkeep window - all of it is the request's;
//   * `STATIC2` is the countdown of the upkeep, refreshed every process: `TID_GAME_GUILDHOUSE_EXPIRATION` once it is over, else the nonzero parts of days, hours, minutes and seconds joined by blanks;
//   * destroying the window closes the storage window, the guild window, the guild bank and ends a guild placement (`request.destroyed()`).

import {
  APP_GH_MAIN,
  WIDC_BTN_FURNITURE,
  WIDC_BTN_SETUPPOWER,
  WIDC_BTN_STORAGE,
  WIDC_BTN_TELEPORTER,
  WIDC_BTN_UPKEEP,
  WIDC_STATIC2,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { GhMainRequest, HousingHost } from './ports';
import { upkeepCountdownText } from './rows';

export class WndGHMainMenu extends WndNeuz {
  readonly appId = APP_GH_MAIN;
  readonly #request: GhMainRequest;
  readonly #host: HousingHost;

  constructor(request: GhMainRequest, host: HousingHost) {
    super();
    this.#request = request;
    this.#host = host;
    this.noCloseButton = true;
  }

  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }
  /** `WIDC_STATIC2`: the countdown. */
  get countdown(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC2);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#updateCountdown();
  }

  #updateCountdown(): void {
    const s = this.countdown;
    if (!s) return;
    s.setTitle(
      upkeepCountdownText(this.#request.upkeepAt(), this.#request.nowSec(), this.#host.text),
    );
  }

  override process(now: number): void {
    super.process(now);
    this.#updateCountdown();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_BTN_SETUPPOWER:
          this.#request.toggleSetupPower();
          return true;
        case WIDC_BTN_FURNITURE:
          this.#request.toggleStorage('furniture');
          return true;
        case WIDC_BTN_STORAGE:
          this.#request.toggleBank();
          return true;
        case WIDC_BTN_TELEPORTER:
          this.#request.toggleStorage('teleporter');
          return true;
        case WIDC_BTN_UPKEEP:
          this.#request.toggleUpkeep();
          return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.destroyed();
    super.onDestroy();
  }
}
