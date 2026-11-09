// `WndRainbowRaceOffer` (`CWndRainbowRaceOffer`, `src:_Interface/WndRainbowRace.cpp:17-68`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-60): `APP_RAINBOWRACE_OFFER` (766, 336 x 480), the application window. `TEXT1` holds
// the number of applicants in bold black (`TID_GAME_RAINBOWRACE_OFFERNUM`) and the panel `rainbowraceoffer` (the fee is in its text); OK sends the application (`request.apply()` = `SendRainbowRaceApplication`)
// and closes, CANCEL closes. Centred.

import { APP_RAINBOWRACE_OFFER, WIDC_CANCEL, WIDC_OK, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import { TID_GAME_RAINBOWRACE_OFFERNUM } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { OfferRequest, RainbowHost } from './ports';

export interface OfferDeps {
  readonly host: RainbowHost;
}

export class WndRainbowRaceOffer extends WndNeuz {
  readonly appId = APP_RAINBOWRACE_OFFER;
  readonly #request: OfferRequest;
  readonly #host: RainbowHost;

  constructor(request: OfferRequest, deps: OfferDeps) {
    super();
    this.#request = request;
    this.#host = deps.host;
    this.putRegInfo = false;
  }

  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `AddString( .. , black, ESSTY_BOLD )`, "\n", `AddParsingString( panel )`
    const head = this.#host.fmt(TID_GAME_RAINBOWRACE_OFFERNUM, this.#request.count);
    this.textCtrl?.setString(`#b#cff000000${head}#nb#nc\n${this.#request.text}`);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.#request.apply();
        this.destroy();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
