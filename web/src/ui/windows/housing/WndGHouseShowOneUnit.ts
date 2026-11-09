// `WndGHouseShowOneUnit` (`CWndGHouseShowOneUnit`, `src:_Interface/WndHousing.cpp:1436-1523`, Plan/42 §19.4 N, 45 §28.10; WP-UI-59): `APP_GH_SHOWONE` (304 x 80), the strip that shows the furniture being placed while the storage window is
// hidden: top-centre of the root rect, NO close button, the icon at (+10, 0) of the list rect, the name at (+50, +12), the time at (+200, +12) followed by ` (NEW)` (the item never ran) or ` (>)` (its time runs), and a red bar
// `0x55ff0000` over the whole list rect that blinks every 600 ms.

import { APP_GH_SHOWONE, WIDC_LISTBOX1 } from '../../../gen/defs/resdata';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { SpriteLayer } from '../../controls/SpriteLayer';
import { TextLayer } from '../../controls/TextLayer';
import { type Rect, rectHeight, rectWidth } from '../../layout/geometry';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { HousingHost } from './ports';
import { COLOR_BLACK, FILL_RED, type GhRowView, SHOWONE_BLINK_MS, showOneTimeText } from './rows';

const ID_TEXT = 0x7a40;
const ID_ICON = 0x7a41;
const ID_BAR = 0x7a42;

export interface ShowOneDeps {
  readonly host: HousingHost;
  readonly nowSec: () => number;
}

export class WndGHouseShowOneUnit extends WndNeuz {
  readonly appId = APP_GH_SHOWONE;
  readonly #deps: ShowOneDeps;
  readonly #item: GhRowView;
  #texts: TextLayer | null = null;
  #icon: SpriteLayer | null = null;
  #bar: RectLayer | null = null;
  /** `bRenderBox`: the bar is drawn. */
  #barOn = true;
  #barAt = Number.NaN;

  constructor(item: GhRowView, deps: ShowOneDeps) {
    super();
    this.#item = item;
    this.#deps = deps;
    this.noCloseButton = true;
    this.putRegInfo = false;
  }

  get item(): GhRowView {
    return this.#item;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  get bar(): RectLayer | null {
    return this.#bar;
  }
  get barVisible(): boolean {
    return this.#barOn;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const list: Rect = this.getWndCtrl(WIDC_LISTBOX1)?.rect ?? { l: 8, t: 8, r: 280, b: 42 };
    const client = this.getClientRect(false);
    this.#bar = this.addControl(new RectLayer(), ID_BAR, client);
    this.#icon = this.addControl(new SpriteLayer(), ID_ICON, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXT, client);
    const t = this.#deps.host.text;
    this.#icon.setEntries([
      {
        key: 'icon',
        path: this.#item.icon === '' ? null : this.#item.icon,
        x: list.l + 10,
        y: list.t,
        w: 32,
        h: 32,
      },
    ]);
    this.#texts.setItems([
      { x: list.l + 50, y: list.t + 12, text: this.#item.name, color: COLOR_BLACK },
      {
        x: list.l + 200,
        y: list.t + 12,
        text: showOneTimeText(this.#item, this.#deps.nowSec(), t),
        color: COLOR_BLACK,
      },
    ]);
    this.#drawBar(list);
    // top-centre of the root rect: `( right * 0.5 - width * 0.5, 0 )`
    const root = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((root.r - root.l) * 0.5 - rectWidth(this.rectWindowValue) * 0.5),
      y: 0,
    });
  }

  #drawBar(list: Rect): void {
    const entries: RectEntry[] = this.#barOn
      ? [
          {
            key: 'bar',
            x: list.l,
            y: list.t,
            w: rectWidth(list),
            h: rectHeight(list),
            fill: FILL_RED,
          },
        ]
      : [];
    this.#bar?.setEntries(entries);
  }

  /** The bar toggles every 600 ms (`g_tmCurrent - tmOld > 600`). */
  override process(now: number): void {
    super.process(now);
    if (Number.isNaN(this.#barAt)) this.#barAt = now;
    if (now - this.#barAt > SHOWONE_BLINK_MS) {
      this.#barAt = now;
      this.#barOn = !this.#barOn;
      this.#drawBar(this.getWndCtrl(WIDC_LISTBOX1)?.rect ?? { l: 8, t: 8, r: 280, b: 42 });
    }
  }
}
