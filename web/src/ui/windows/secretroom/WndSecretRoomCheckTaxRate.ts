// `WndSecretRoomCheckTaxRate` (`CWndSecretRoomCheckTaxRate`, `src:_Interface/WndSecretRoom.cpp:699-822`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_TAXRATE_CHECK` (716,
// 336 x 384), the tax check of the area the player stands in. `STATIC3` / `STATIC4` show the sales and purchase rates ("%d%%", the integers of the player's area, 0 without one), `STATIC8` the
// occupying guild and `STATIC9` its master, both cut over 16 characters with "..."; without an owner (or for a guild the client does not know) both read "Unoccupied". The master's
// name follows the player-data cache (the request's view). `WIDC_TEXT1` shows the panel `secretroomchecktaxrate`. OK closes.

import {
  APP_SECRETROOM_TAXRATE_CHECK,
  WIDC_OK,
  WIDC_STATIC3,
  WIDC_STATIC4,
  WIDC_STATIC8,
  WIDC_STATIC9,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_GAME_SECRETROOM_TEX_NOGUILD } from '../../../gen/defs/text';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { CheckTaxRequest, SecretRoomHost } from './ports';
import { cutOver, percentText } from './rows';

export interface CheckTaxDeps {
  readonly host: SecretRoomHost;
}

export class WndSecretRoomCheckTaxRate extends WndNeuz {
  readonly appId = APP_SECRETROOM_TAXRATE_CHECK;
  readonly #request: CheckTaxRequest;
  readonly #host: SecretRoomHost;
  #unsubscribe: (() => void) | null = null;

  constructor(request: CheckTaxRequest, deps: CheckTaxDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get salesStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC3);
  }
  get purchaseStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC4);
  }
  get guildStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC8);
  }
  get masterStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC9);
  }
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    this.salesStatic?.setTitle(percentText(r.salesRate));
    this.purchaseStatic?.setTitle(percentText(r.purchaseRate));
    this.#showOwner();
    this.#unsubscribe = r.master.subscribe(() => {
      this.#showOwner();
    });
    if (r.info !== '') this.textCtrl?.setString(r.info);
    this.moveParentCenter();
  }

  #showOwner(): void {
    const r = this.#request;
    if (r.guild === null) {
      const none = this.#host.text(TID_GAME_SECRETROOM_TEX_NOGUILD);
      this.guildStatic?.setTitle(none);
      this.masterStatic?.setTitle(none);
      return;
    }
    this.guildStatic?.setTitle(cutOver(r.guild, 16));
    this.masterStatic?.setTitle(cutOver(r.master.get() ?? '', 16));
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    this.#request.closed?.();
    super.onDestroy();
  }
}
