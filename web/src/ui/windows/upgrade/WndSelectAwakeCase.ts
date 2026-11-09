// `WndSelectAwakeCase` (`CWndSelectAwakeCase`, `src:_Interface/WndAwakening.cpp:196-400`, `__PROTECT_AWAKE`, ref/22 §17 L, Plan/45 §18.4; WP-UI-31 /
// WP-CFEAT-25): `APP_AWAKE_SELECTCASE` (400 x 256, MODAL), the choice of a protected awakening. The awakened item's icon stands at the left
// (`WIDC_STATIC1`), the lines of the OLD option set are listed in the upper box and the lines of the NEW set in the lower one (black, 20 px apart, from
// x = 70), the two select buttons (`WIDC_BUTTON1` keeps the old set, `WIDC_BUTTON2` takes the new one) sit at the right and a bar under them
// runs down over `AWAKE_KEEP_TIME` (60 s). A button sends the choice and closes; when the time is up the window sends "keep the old set" itself and closes.
//
// The theme pictures of the buttons, the option boxes and the bar (`Buttselect01.tga`, `Wndoption.tga`, `Wndguage.tga`, `Wndguagebar.tga`) are not in the
// Resource, so the original shows no art there: the buttons fall back to the plain push button (labelled "Keep" and "Select" here, the original has no
// text) and the bar is a red fill.

import {
  APP_AWAKE_SELECTCASE,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_STATIC1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Rect } from '../../layout/geometry';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import { KEEP_LABEL, TAKE_LABEL } from './names';
import type { SelectAwakeRequest } from './ports';
import { NO_RECT } from './SlotWindow';

/** The x of the option lines and the y of the first line of each box (`TextOut( 70, 20 + i * 20 )` and `92 + i * 20`, each after a leading line feed). */
const TEXT_X = 70;
const OLD_Y = 20;
const NEW_Y = 92;
const LINE_STEP = 20;
/** The leading `"\n"` of every option string moves its text down one font line. */
const LEAD = 14;
/** The bar: `rect.left += 14`, `rect.top += 174`, 20 px high, 360 px at the start. */
const BAR = { x: 14, y: 174, w: 360, h: 20 } as const;
const BAR_FILL = 'rgba(255, 0, 0, 1)';

export class WndSelectAwakeCase extends WndNeuz {
  readonly appId = APP_AWAKE_SELECTCASE;
  readonly #request: SelectAwakeRequest;
  #icon: PaperDoll | null = null;
  #text: TextLayer | null = null;
  #bar: RectLayer | null = null;
  #started = 0;
  #done = false;
  #iconRect: Rect = NO_RECT;

  constructor(request: SelectAwakeRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  get keepButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get takeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get textLayer(): TextLayer | null {
    return this.#text;
  }
  get barLayer(): RectLayer | null {
    return this.#bar;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icon;
  }
  /** The fraction of the time left, `0..1`. */
  get remaining(): number {
    const total = this.#request.durationMs;
    if (total <= 0) return 0;
    const left = total - (this.#request.now() - this.#started);
    return Math.max(0, Math.min(1, left / total));
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#started = this.#request.now();
    this.#iconRect = this.getWndCtrl(WIDC_STATIC1)?.rect ?? NO_RECT;
    const client = this.getClientRect(false);
    this.#icon = this.addControl(new PaperDoll(), 0x7e56, client);
    this.#bar = this.addControl(new RectLayer(), 0x7e57, client);
    this.#text = this.addControl(new TextLayer(), 0x7e58, client);
    this.keepButton?.setTitle(KEEP_LABEL);
    this.takeButton?.setTitle(TAKE_LABEL);
    this.#icon.setLayers(
      [
        {
          key: 'item',
          x: this.#iconRect.l,
          y: this.#iconRect.t,
          size: 32,
          icon: this.#request.item.icon,
          alpha: 1,
          tint: false,
        },
      ],
      true,
    );
    const items: TextItem[] = [];
    this.#request.oldLines.forEach((line, i) => {
      items.push({ x: TEXT_X, y: OLD_Y + LEAD + i * LINE_STEP, text: line, color: 0xff000000 });
    });
    this.#request.newLines.forEach((line, i) => {
      items.push({ x: TEXT_X, y: NEW_Y + LEAD + i * LINE_STEP, text: line, color: 0xff000000 });
    });
    this.#text.setItems(items);
    this.#paintBar();
    this.moveParentCenter();
  }

  /** `OnDraw`: the clock runs; at `AWAKE_KEEP_TIME` the old set is kept. */
  override process(now: number): void {
    super.process(now);
    if (this.#done) return;
    if (this.#request.now() - this.#started >= this.#request.durationMs) {
      this.#done = true;
      this.#request.timeout();
      this.destroy();
      return;
    }
    this.#paintBar();
  }

  /** A button: the choice is sent once and the window closes. */
  choose(flag: 1 | 2): void {
    if (this.#done) return;
    this.#done = true;
    this.#request.choose(flag);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.choose(1);
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON2) {
      this.choose(2);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }

  #paintBar(): void {
    const w = Math.trunc(BAR.w * this.remaining);
    const entries: RectEntry[] =
      w > 0 ? [{ key: 'time', x: BAR.x, y: BAR.y, w, h: BAR.h, fill: BAR_FILL }] : [];
    this.#bar?.setEntries(entries);
  }
}
