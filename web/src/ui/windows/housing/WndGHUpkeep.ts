// `WndGHUpkeep` (`CWndGHUpkeep`, `src:_Interface/WndGuildHouse.cpp:635-816`, Plan/42 §19.4 N, 45 §28.10; WP-UI-58): the upkeep box of the guild house on the layout `APP_CONFIRM_BUYEX` (306, `APP_CONFIRM_BUY_` of the C++ is its alias).
// Title `TID_GAME_GUILDHOUSE_TAXUI01`, `STATIC1` `TAXUI03` (the amount), `CONTROL1` `TAXUI02` (the duration), `STATIC2` the cost in the money style. The days start at 1; `PLUS` / `MINUS` / `MAX` stay within [1, 34]
// (`MAX_UPKEEP_DAY - 1`), the edit is clamped to [1, 34] and the cost is ALWAYS recomputed (FIX: the original stopped updating above 34 and could send 0), the cost is `days * 2 400 000`. `OK` pays and closes, `CANCEL` closes.
// It opens under its OWN instance key (`wm.open( APP_CONFIRM_BUYEX, build, 'ghUpkeep' )`), so it never collides with the shop's quantity box.

import {
  APP_CONFIRM_BUYEX,
  WIDC_CANCEL,
  WIDC_CONTROL1,
  WIDC_CONTROL2,
  WIDC_MAX,
  WIDC_MINUS,
  WIDC_OK,
  WIDC_PLUS,
  WIDC_STATIC,
  WIDC_STATIC1,
  WIDC_STATIC2,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDHOUSE_TAXUI01,
  TID_GAME_GUILDHOUSE_TAXUI02,
  TID_GAME_GUILDHOUSE_TAXUI03,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_NUMBER, WSS_MONEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { atoi } from '../npc/quantity';
import type { HousingHost, UpkeepRequest } from './ports';
import { clampUpkeepDays, MAX_UPKEEP_DAY_CLIENT, UPKEEP_KEY, upkeepCost } from './rows';

export class WndGHUpkeep extends WndNeuz {
  readonly appId = APP_CONFIRM_BUYEX;
  override readonly instanceKey = UPKEEP_KEY;
  readonly #request: UpkeepRequest;
  readonly #host: HousingHost;
  #edit: EditCtrl | null = null;
  #cost: Static | null = null;

  constructor(request: UpkeepRequest, host: HousingHost) {
    super();
    this.#request = request;
    this.#host = host;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  /** `WIDC_STATIC2`: the cost. */
  get costText(): Static | null {
    return this.#cost;
  }
  button(id: number): Button | null {
    return this.getDlgItem<Button>(id);
  }
  /** The days in the edit. */
  get days(): number {
    return atoi(this.#edit?.getString() ?? '');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const t = this.#host.text;
    this.setTitle(t(TID_GAME_GUILDHOUSE_TAXUI01));
    this.getDlgItem<Static>(WIDC_STATIC)?.setTitle('');
    this.getDlgItem<Static>(WIDC_STATIC1)?.setTitle(t(TID_GAME_GUILDHOUSE_TAXUI03));
    this.getDlgItem<Static>(WIDC_CONTROL1)?.setTitle(t(TID_GAME_GUILDHOUSE_TAXUI02));
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_CONTROL2);
    this.#cost = this.getDlgItem<Static>(WIDC_STATIC2);
    this.#cost?.addWndStyle(WSS_MONEY);
    this.#edit?.addWndStyle(EBS_NUMBER);
    this.moveParentCenter();
    this.#change(1);
  }

  /** `OnChangeBuyCount`: the edit shows `n` (clamped to [1, 34]), the cost follows. */
  #change(n: number): void {
    const days = clampUpkeepDays(n);
    if (this.#edit && atoi(this.#edit.getString()) !== days) this.#edit.setString(String(days));
    this.#cost?.setTitle(String(upkeepCost(days)));
  }

  /** `OnOK`: the request pays the days of the edit. */
  ok(): void {
    this.#request.pay(clampUpkeepDays(this.days));
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_CHANGE && id === WIDC_CONTROL2) {
      this.#change(Math.max(this.days, 0));
      return true;
    }
    if (msg === Wnm.EN_RETURN) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_PLUS:
          this.#change(Math.min(this.days + 1, MAX_UPKEEP_DAY_CLIENT));
          return true;
        case WIDC_MINUS:
          this.#change(Math.max(this.days - 1, 1));
          return true;
        case WIDC_MAX:
          this.#change(MAX_UPKEEP_DAY_CLIENT);
          return true;
        case WIDC_CANCEL:
          this.destroy();
          return true;
        case WIDC_OK:
          this.ok();
          return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
