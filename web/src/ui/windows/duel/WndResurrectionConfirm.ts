// `WndResurrectionConfirm` (`CWndResurrectionConfirm`, `src:_Interface/WndField.cpp:13307-13370`, ref/22 §17 C, Plan/42 §19.4 C, 45 §25.4;
// WP-UI-41): `APP_RESURRECTION` (288 x 144), the offer of another player's Resurrection skill. The two layout statics carry the words
// ("Another player is trying to resurrect you." / "Do you want to be resurrected?"); `WIDC_BUTTON1` is Yes (`ResurrectionOk`),
// `WIDC_BUTTON2` is No (`ResurrectionCancel`). The window is modal, has no title bar (`m_wndTitleBar.SetVisible(FALSE)`; the id 10000 of its
// close button still cancels) and opens centred horizontally, 70 px from the top. Enter is Yes (Plan/42 WP-UI-41).

import { APP_RESURRECTION, WIDC_BUTTON1, WIDC_BUTTON2 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_KEY, WBS_MODAL, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DUEL_WINDOW_Y } from './geometry';
import type { ResurrectionRequest } from './ports';

export class WndResurrectionConfirm extends WndNeuz {
  readonly appId = APP_RESURRECTION;
  readonly #request: ResurrectionRequest;
  #answered = false;

  constructor(request: ResurrectionRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_RESURRECTION, WBS_MODAL)`; the keyboard on the frame so that Enter presses the default Yes button. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get answered(): boolean {
    return this.#answered;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.titleBar?.setVisible(false);
    this.yesButton?.setDefault(true);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: DUEL_WINDOW_Y,
    });
  }

  /** Yes / Enter: `ResurrectionOk`, then the window closes. */
  yes(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.yes();
    this.destroy();
  }

  /** No / the close button id: `ResurrectionCancel`, then the window closes. */
  no(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.no();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.yes();
        return true;
      }
      if (id === WIDC_BUTTON2 || id === WTBID_CLOSE) {
        this.no();
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
