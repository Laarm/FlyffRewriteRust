// `WndSecretRoomChangeTaxRate` (`CWndSecretRoomChangeTaxRate`, `src:_Interface/WndSecretRoom.cpp:546-698`, ref/22 §17 K, Plan/42 §19.4 K, 45 §28.3; WP-UI-56): `APP_SECRETROOM_TAXRATE_CHANGE` (715,
// 384 x 368), the window in which the master of the winning guild sets the sales and purchase tax rates of the continent. `STATIC4` / `STATIC6` show the CURRENT rates ("%d%%", the integers of the
// tax slice), `STATIC5` / `STATIC7` the new ones; the presets are `max( current, min )` (KEEP: not clamped to the maximum, `SetDefaultTax`), `PLUS_*` adds one up to the maximum and `MINUS_*`
// subtracts one down to the minimum (the original's clamps). `WIDC_TEXT1` shows the panel `secretroomchangetaxrate`. OK asks "Would you like to set Sales Tax N%, Purchase Tax M%?"
// (`CHANGETEX`, OK / Cancel); the box's OK hands the rates to the request and closes THIS window at once (the later `TaxAllInfo` finds it closed); Cancel closes the box only.

import {
  APP_SECRETROOM_TAXRATE_CHANGE,
  WIDC_MINUS_PTAX,
  WIDC_MINUS_STAX,
  WIDC_OK,
  WIDC_PLUS_PTAX,
  WIDC_PLUS_STAX,
  WIDC_STATIC4,
  WIDC_STATIC5,
  WIDC_STATIC6,
  WIDC_STATIC7,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import { TID_GAME_SECRETROOM_CHANGETEX } from '../../../gen/defs/text';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { ChangeTaxRequest, SecretRoomHost } from './ports';
import { percentText, stepRate } from './rows';

export interface ChangeTaxDeps {
  readonly host: SecretRoomHost;
}

export class WndSecretRoomChangeTaxRate extends WndNeuz {
  readonly appId = APP_SECRETROOM_TAXRATE_CHANGE;
  readonly #request: ChangeTaxRequest;
  readonly #host: SecretRoomHost;
  /** `m_nChangeSalesTax`, `m_nChangePurchaseTax`. */
  #sales = 0;
  #purchase = 0;

  constructor(request: ChangeTaxRequest, deps: ChangeTaxDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.#sales = request.salesPreset;
    this.#purchase = request.purchasePreset;
    this.putRegInfo = false;
  }

  /** The continent the window sets (15 East, 240 West). */
  get continent(): number {
    return this.#request.continent;
  }
  get newSales(): number {
    return this.#sales;
  }
  get newPurchase(): number {
    return this.#purchase;
  }
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get salesNow(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC4);
  }
  get salesNew(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC5);
  }
  get purchaseNow(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC6);
  }
  get purchaseNew(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC7);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    this.salesNow?.setTitle(percentText(r.salesNow));
    this.purchaseNow?.setTitle(percentText(r.purchaseNow));
    this.#show();
    if (r.info !== '') this.textCtrl?.setString(r.info);
    this.moveParentCenter();
  }

  #show(): void {
    this.salesNew?.setTitle(percentText(this.#sales));
    this.purchaseNew?.setTitle(percentText(this.#purchase));
  }

  /** `WIDC_PLUS_STAX` / `WIDC_MINUS_STAX`. */
  stepSales(dir: -1 | 1): void {
    this.#sales = stepRate(this.#sales, dir, this.#request.min, this.#request.max);
    this.#show();
  }

  /** `WIDC_PLUS_PTAX` / `WIDC_MINUS_PTAX`. */
  stepPurchase(dir: -1 | 1): void {
    this.#purchase = stepRate(this.#purchase, dir, this.#request.min, this.#request.max);
    this.#show();
  }

  /** `WIDC_OK`: the confirmation; its OK sends the rates and closes this window. */
  ok(): void {
    const sales = this.#sales;
    const purchase = this.#purchase;
    this.#host.confirm(
      this.#host.fmt(TID_GAME_SECRETROOM_CHANGETEX, sales, purchase),
      () => {
        this.#request.setRate(this.#request.continent, sales, purchase);
        this.destroy();
      },
      this,
    );
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_PLUS_STAX:
          this.stepSales(1);
          return true;
        case WIDC_MINUS_STAX:
          this.stepSales(-1);
          return true;
        case WIDC_PLUS_PTAX:
          this.stepPurchase(1);
          return true;
        case WIDC_MINUS_PTAX:
          this.stepPurchase(-1);
          return true;
        case WIDC_OK:
          this.ok();
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
