// `WndStatus` (`CWndStatus`, `src:_Interface/WndField.cpp:10637-11830`, ref/22 §17 C, Plan/42 §19.4 C, 45 §7.2; WP-UI-21):
// `APP_STATUS1` (220 x 112, the picture `WndStatus2.tga`, which already holds the empty gauge frames). The window adds
//   * the five fill gauges over `WIDC_CUSTOM1..5` - HP red, MP blue, FP green (`GauEmptyNormal.bmp`), EXP and the guild-house
//     rest bonus (`GauEmptySmall.bmp`) - as vertex-colour-modulated three-slice bars (`MakeGaugeVertex`);
//   * the numbers over them in the outlined title font, in the display mode a click cycles (2 -> 0 -> 1 -> 2);
//   * the title "name - Lv: n" (the original paints the title itself in `PaintFrame`);
//   * the head portrait canvas over `WIDC_CUSTOM6` (41's `statusPortrait` preview) and the EXP stop button.
// The view is polled by `process` (10 Hz): the gauges and the texts are patched with diffing setters, so an unchanged view
// commits nothing to React.

import {
  APP_STATUS1,
  WIDC_BUTTON1,
  WIDC_CUSTOM1,
  WIDC_CUSTOM2,
  WIDC_CUSTOM3,
  WIDC_CUSTOM4,
  WIDC_CUSTOM5,
  WIDC_CUSTOM6,
} from '../../../gen/defs/resdata';
import { TID_TOOLTIP_RESTPOINT_GAGE } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { Gauge } from '../../controls/Gauge';
import { TextLayer } from '../../controls/TextLayer';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import type { Shortcut } from '../../dnd/shortcut';
import { WndNeuz } from '../../wnd/WndNeuz';
import type {
  CorePreviewHandle,
  CorePreviewPort,
  StatusActions,
  StatusView,
  ViewSource,
} from './ports';
import {
  nextStatusDisplay,
  STATUS_DISPLAY_INITIAL,
  type StatusDisplay,
  type StatusRects,
  statusTexts,
} from './statusText';

/** The vertex colours of `MakeGaugeVertex` (`0xAARRGGBB`): alpha 100 for the three points, 0x84 for the experience bars. */
export const GAUGE_COLORS = {
  hp: 0x64ff0000,
  mp: 0x640000ff,
  fp: 0x6400ff00,
  exp: 0x847070ff,
  rest: 0x84e6ce19,
} as const;
/** `m_texGauFillNormal` / `m_texGauFillSmall` (both are loaded from the "Empty" files). */
export const GAUGE_TEXTURE_NORMAL = 'GauEmptyNormal.bmp';
export const GAUGE_TEXTURE_SMALL = 'GauEmptySmall.bmp';
/** The window ids of the code-created controls. */
const ID_TEXTS = 0x7a10;
const ID_GAUGE = 0x7a11;

export interface StatusDeps {
  readonly view: ViewSource<StatusView>;
  readonly actions: StatusActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  readonly previews?: CorePreviewPort | null | undefined;
}

const EMPTY: StatusView = {
  title: '',
  hp: 0,
  hpMax: 0,
  mp: 0,
  mpMax: 0,
  fp: 0,
  fpMax: 0,
  expPercentX100: 0,
  restPoint: 0,
  restText: false,
  improved: true,
  expStopFeature: true,
  expStopped: false,
};

export class WndStatus extends WndNeuz {
  readonly appId = APP_STATUS1;
  readonly #deps: StatusDeps;
  #display: StatusDisplay = STATUS_DISPLAY_INITIAL;
  #rects: StatusRects | null = null;
  #hp: Gauge | null = null;
  #mp: Gauge | null = null;
  #fp: Gauge | null = null;
  #exp: Gauge | null = null;
  #rest: Gauge | null = null;
  #texts: TextLayer | null = null;
  #expStop: Button | null = null;
  #preview: CorePreviewHandle | null = null;
  #applied: StatusView | null = null;
  #view: StatusView = EMPTY;

  constructor(deps: StatusDeps) {
    super();
    this.#deps = deps;
  }

  get gauges(): { hp: Gauge; mp: Gauge; fp: Gauge; exp: Gauge; rest: Gauge } | null {
    const { hp, mp, fp, exp, rest } = {
      hp: this.#hp,
      mp: this.#mp,
      fp: this.#fp,
      exp: this.#exp,
      rest: this.#rest,
    };
    return hp && mp && fp && exp && rest ? { hp, mp, fp, exp, rest } : null;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  get display(): StatusDisplay {
    return this.#display;
  }
  get expStopButton(): Button | null {
    return this.#expStop;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rectOf = (id: number): Rect => this.getWndCtrl(id)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
    const rects: StatusRects = {
      hp: rectOf(WIDC_CUSTOM1),
      mp: rectOf(WIDC_CUSTOM2),
      fp: rectOf(WIDC_CUSTOM3),
      exp: rectOf(WIDC_CUSTOM4),
      rest: rectOf(WIDC_CUSTOM5),
    };
    this.#rects = rects;
    const gauge = (r: Rect, texture: string, color: number): Gauge =>
      this.addControl(new Gauge({ fill: texture, color }), ID_GAUGE, r);
    // the draw order of the original: HP, MP, FP, EXP, rest bonus
    this.#hp = gauge(rects.hp, GAUGE_TEXTURE_NORMAL, GAUGE_COLORS.hp);
    this.#mp = gauge(rects.mp, GAUGE_TEXTURE_NORMAL, GAUGE_COLORS.mp);
    this.#fp = gauge(rects.fp, GAUGE_TEXTURE_NORMAL, GAUGE_COLORS.fp);
    this.#exp = gauge(rects.exp, GAUGE_TEXTURE_SMALL, GAUGE_COLORS.exp);
    this.#rest = gauge(rects.rest, GAUGE_TEXTURE_SMALL, GAUGE_COLORS.rest);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, this.getClientRect(false));
    this.#expStop = this.getDlgItem<Button>(WIDC_BUTTON1);

    const canvas = this.customCanvas(WIDC_CUSTOM6);
    if (canvas && this.#deps.previews) {
      this.#preview = this.#deps.previews.open({
        canvas,
        preset: 'statusPortrait',
        subject: this.#deps.view.get().subject,
      });
    }
    // the window opens in the top-left corner of the layout rect
    const layout = this.env.wm.layoutRect;
    this.move({ x: layout.l, y: layout.t });
    this.#apply(this.#deps.view.get());
  }

  // ---- the view ----------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: StatusView): void {
    this.#applied = v;
    this.#view = v;
    this.setTitle(v.title);
    this.#hp?.set(v.hp, v.hpMax);
    this.#mp?.set(v.mp, v.mpMax);
    this.#fp?.set(v.fp, v.fpMax);
    this.#exp?.set(v.expPercentX100, 10000);
    // the rest bonus: its value is already a width in px; 1..8 px are drawn as 9 (`MakeGaugeVertex` needs two slices)
    const width = this.#rects ? this.#rects.rest.r - this.#rects.rest.l : 0;
    const rest = v.restPoint > 0 && v.restPoint < 9 ? 9 : v.restPoint;
    this.#rest?.set(Math.min(rest, width), width);
    this.#expStop?.setVisible(v.expStopFeature);
    this.#expStop?.setCheck(v.expStopped);
    this.#paintTexts();
    this.#preview?.invalidate();
  }

  #paintTexts(): void {
    if (!this.#rects) return;
    this.#texts?.setItems(
      statusTexts({ view: this.#view, display: this.#display, rects: this.#rects }),
    );
  }

  // ---- input --------------------------------------------------------------------------------------------------------

  /** `OnLButtonDown`: a click on the window cycles the display mode. */
  override onLButtonDown(): void {
    this.#display = nextStatusDisplay(this.#display);
    this.#paintTexts();
  }

  /** `OnMouseWndSurface`: the rest-bonus gauge has a tooltip. */
  override onMouseMove(pt: Point): void {
    const r = this.#rects?.rest;
    if (!r || !rectContains(r, pt) || this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(
      this,
      this.#deps.getText(TID_TOOLTIP_RESTPOINT_GAGE),
      rectOffset(r, s.l, s.t),
      0,
    );
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.#deps.actions.expStop();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#preview?.close();
    this.#preview = null;
    super.onDestroy();
  }
}
