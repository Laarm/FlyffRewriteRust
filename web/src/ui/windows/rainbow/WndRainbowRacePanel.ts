// `WndRainbowRacePanel` (`CWndRainbowRaceInfo`, `CWndRainbowRaceRule`, `CWndRainbowRacePrize`, `CWndRainbowRaceMiniGameInfo`, `src:_Interface/WndRainbowRace.cpp:74-162, 274-315, 611-675`, ref/22 §17 P, Plan/42
// §19.4 P; WP-UI-60): ONE class for the four read-only windows with a panel text in `TEXT1` and an OK button that closes: `APP_RAINBOWRACE_INFO` (768), `_RULE` (770), `_PRIZE` (774), all 368 x 368 and
// centred, and `_MINIGAME_INFO` (794, 304 x 400), which opens beside the progress window (`infoPosition`) and goes with it.

import {
  APP_RAINBOWRACE_MINIGAME,
  APP_RAINBOWRACE_MINIGAME_INFO,
  WIDC_OK,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { infoPosition } from './geometry';
import type { PanelRequest } from './ports';

export class WndRainbowRacePanel extends WndNeuz {
  readonly appId: number;
  readonly #request: PanelRequest;

  /** `app`: 768 info, 770 rules, 774 prizes or 794 game information. */
  constructor(app: number, request: PanelRequest) {
    super();
    this.appId = app;
    this.#request = request;
    this.putRegInfo = false;
  }

  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.textCtrl?.setString(this.#request.text);
    if (this.appId !== APP_RAINBOWRACE_MINIGAME_INFO) {
      this.moveParentCenter();
      return;
    }
    // beside the progress window when it is open (`GetWndBase( APP_RAINBOWRACE_MINIGAME )`)
    const progress = this.env.wm.get(APP_RAINBOWRACE_MINIGAME);
    if (!progress) return;
    const at = infoPosition(this.env.wm.layoutRect, progress.rectWindow, {
      x: rectWidth(this.rectWindow),
      y: rectHeight(this.rectWindow),
    });
    this.move(at);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
