// `WndRevival` (`CWndRevival`, `src:_Interface/WndField.cpp:12642-12830`, ref/22 §17 C, Plan/42 §19.4 C, 45 §25.3; WP-UI-22):
// `APP_REVIVAL` (176 x 128, modal, opened centred at the top when the player dies). It has no title bar and is exempt from
// Esc (`ESC_SKIP_APPS`). Buttons: to the lodestar, "revive here" (needs a resurrection scroll), the shop; the lodelight button
// is disabled and hidden since V9 and "revive here" takes its place, the shop takes the place of "revive here". Every
// button but the shop sends its revival and closes the window.

import {
  APP_RESURRECTION,
  APP_REVIVAL,
  WIDC_REVIVAL_SHOP,
  WIDC_REVIVAL_STAND,
  WIDC_REVIVAL_TO_LODELIGHT,
  WIDC_REVIVAL_TO_LODESTAR,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_TOOLTIP_LODELIGHT,
  TID_GAME_TOOLTIP_LODESTAR,
  TID_GAME_TOOLTIP_OGNPOINT,
  TID_GAME_TOOLTIP_SHOP,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { RevivalChoice, RevivalRequest, RevivalView } from './ports';

export interface RevivalDeps {
  readonly request: RevivalRequest;
  readonly getText: (tid: number) => string;
}

export class WndRevival extends WndNeuz {
  readonly appId = APP_REVIVAL;
  readonly #deps: RevivalDeps;
  #lodelight: Button | null = null;
  #lodestar: Button | null = null;
  #stand: Button | null = null;
  #shop: Button | null = null;
  #applied: RevivalView | null = null;

  constructor(deps: RevivalDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_REVIVAL, WBS_MODAL)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  get lodelightButton(): Button | null {
    return this.#lodelight;
  }
  get lodestarButton(): Button | null {
    return this.#lodestar;
  }
  get standButton(): Button | null {
    return this.#stand;
  }
  get shopButton(): Button | null {
    return this.#shop;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#lodelight = this.getDlgItem<Button>(WIDC_REVIVAL_TO_LODELIGHT);
    this.#lodestar = this.getDlgItem<Button>(WIDC_REVIVAL_TO_LODESTAR);
    this.#stand = this.getDlgItem<Button>(WIDC_REVIVAL_STAND);
    this.#shop = this.getDlgItem<Button>(WIDC_REVIVAL_SHOP);
    const t = this.#deps.getText;
    this.#lodelight?.setToolTip(t(TID_GAME_TOOLTIP_LODELIGHT));
    this.#lodestar?.setToolTip(t(TID_GAME_TOOLTIP_LODESTAR));
    this.#stand?.setToolTip(t(TID_GAME_TOOLTIP_OGNPOINT));
    this.#shop?.setToolTip(t(TID_GAME_TOOLTIP_SHOP));
    // no title bar: the window cannot be closed, only answered
    this.titleBar?.setVisible(false);
    // V9 layout: "revive here" moves into the (hidden) lodelight rect, the shop into the old "revive here" rect
    if (this.#lodelight && this.#stand && this.#shop) {
      const standRect = { ...this.#stand.rectWindow };
      const lodelightRect = { ...this.#lodelight.rectWindow };
      this.#stand.setWndRect(lodelightRect, true);
      this.#shop.setWndRect(standRect, true);
      this.#lodelight.enableWindow(false);
      this.#lodelight.setVisible(false);
    }
    // centred, 70 px from the top (over the gauges)
    const layout = this.env.wm.layoutRect;
    this.move({ x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2), y: 70 });
    this.#apply(this.#deps.request.view.get());
  }

  override process(now: number): void {
    const v = this.#deps.request.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: RevivalView): void {
    this.#applied = v;
    this.#stand?.enableWindow(v.standEnabled);
    this.#lodestar?.enableWindow(v.lodestarEnabled);
    // the lodelight button stays disabled and hidden (V9)
    this.#shop?.setVisible(v.shopVisible);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const choice: RevivalChoice | null =
        id === WIDC_REVIVAL_TO_LODELIGHT
          ? 'lodelight'
          : id === WIDC_REVIVAL_TO_LODESTAR
            ? 'lodestar'
            : id === WIDC_REVIVAL_STAND
              ? 'stand'
              : null;
      const a = this.#deps.request.actions;
      if (choice !== null) {
        a.revive(choice);
        if (this.env.wm.isOpen(APP_RESURRECTION)) a.cancelResurrection();
        this.destroy();
        return true;
      }
      if (id === WIDC_REVIVAL_SHOP) {
        a.shop(); // the window stays open
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
