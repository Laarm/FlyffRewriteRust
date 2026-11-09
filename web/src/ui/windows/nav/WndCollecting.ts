// `WndCollecting` (`CCollectingWnd`, `src:_Interface/WndCollecting.cpp`; ref/22 §17 L, ref/99 §4.7, Plan/42 WP-UI-36, 45 §9.8):
// `APP_COLLECTING` (160 x 208), the panel of the collector weapon. The level `"<opt>  /  5"` of the collector, its battery as a gauge with
// the text `"%d / %d (%d%%)"`, and the start / stop button (`ButtStart.bmp` / `Buttstop.bmp`). The button sends
// `QueryStartCollecting` (refused while flying with `TID_GAME_COLLECT_FLY`) or `QueryStopCollecting` through the port; the picture of the
// button follows `StartCollecting` / `StopCollecting`. The window opens at the right edge of the layout rect, 110 px from the top, and
// is not closed by Esc (`ESC_KEEP_APPS`).

import {
  APP_COLLECTING,
  WIDC_BUTTON1,
  WIDC_GAUGE,
  WIDC_PIC1,
  WIDC_PIC2,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_COLLECTOR_BETTERY_INFO,
  TID_GAME_COLLECTOR_LEVEL_INFO,
} from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import { Gauge } from '../../controls/Gauge';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import { TextLayer } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset, rectWidth } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { type CollectingPorts, type CollectingView, EMPTY_COLLECTING_VIEW } from './ports';

export const COLLECTOR_LEVEL_MAX = 5;
/** The blue of the gauge fill (`D3DCOLOR_ARGB( 128, 0, 0, 255 )`) and of its text. */
export const GAUGE_COLOR = 0x800000ff;
export const GAUGE_TEXT_COLOR = 0xff0000ff;

/** `wsprintf( "%d / %d (%d%%)", remain, max, (int)(ratio * 100) )`, the remain clamped at 0. */
export function batteryText(battery: number, max: number): string {
  const remain = Math.max(0, battery);
  const ratio = max > 0 ? battery / max : 0;
  return `${String(remain)} / ${String(max)} (${String(Math.trunc(ratio * 100))}%)`;
}

/** `"%d  /  5"`: the level line (two spaces around the slash). */
export function levelText(level: number): string {
  return `${String(level)}  /  ${String(COLLECTOR_LEVEL_MAX)}`;
}

export interface CollectingDeps {
  readonly ports: CollectingPorts;
  readonly tooltip: Pick<TooltipService, 'put'>;
  readonly getText: (tid: number) => string;
}

export class WndCollecting extends WndNeuz {
  readonly appId = APP_COLLECTING;
  readonly #deps: CollectingDeps;
  #button: Button | null = null;
  #level: Static | null = null;
  #gauge!: Gauge;
  #sprites!: SpriteLayer;
  #texts!: TextLayer;
  #batteryRect: Rect | null = null;
  #levelRect: Rect | null = null;
  #applied: CollectingView | null = null;
  #started = false;

  constructor(deps: CollectingDeps) {
    super();
    this.#deps = deps;
  }

  get gauge(): Gauge {
    return this.#gauge;
  }
  get startButton(): Button | null {
    return this.#button;
  }
  get levelStatic(): Static | null {
    return this.#level;
  }
  get textLayer(): TextLayer {
    return this.#texts;
  }
  get spriteLayer(): SpriteLayer {
    return this.#sprites;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#button = this.getDlgItem<Button>(WIDC_BUTTON1);
    this.#level = this.getDlgItem<Static>(WIDC_STATIC3);
    this.#batteryRect = this.getWndCtrl(WIDC_STATIC1)?.rect ?? null;
    this.#levelRect = this.getWndCtrl(WIDC_STATIC2)?.rect ?? null;
    const gaugeRect = this.getWndCtrl(WIDC_GAUGE)?.rect ?? { l: 22, t: 120, r: 134, b: 134 };
    this.#gauge = this.addControl(
      new Gauge({
        background: 'GauEmptyNormal.bmp',
        fill: 'GauEmptyNormal.bmp',
        color: GAUGE_COLOR,
      }),
      0x7a21,
      gaugeRect,
    );
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), 0x7a22, client);
    this.#texts = this.addControl(new TextLayer(), 0x7a23, client);
    // `Icon_ColLevel.tga` at `WIDC_PIC1`, `Icon_ColBattery.tga` at `WIDC_PIC2` (natural size)
    const pic1 = this.getWndCtrl(WIDC_PIC1)?.rect;
    const pic2 = this.getWndCtrl(WIDC_PIC2)?.rect;
    this.#sprites.setEntries([
      ...(pic1
        ? [{ key: 'level', path: 'icon/icon_collevel.tga', x: pic1.l, y: pic1.t, w: 24, h: 24 }]
        : []),
      ...(pic2
        ? [{ key: 'battery', path: 'icon/icon_colbattery.tga', x: pic2.l, y: pic2.t, w: 24, h: 24 }]
        : []),
    ]);
    this.#applyView(this.#deps.ports.view.get());
    // the window docks to the right edge of the layout rect, 110 px from the top
    const layout = this.env.wm.layoutRect;
    this.move({ x: layout.r - rectWidth(this.rectWindowValue), y: 110 });
  }

  // ---- the view ---------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    super.process(now);
    const v = this.#deps.ports.view.get();
    if (v !== this.#applied) this.#applyView(v);
  }

  #applyView(v: CollectingView): void {
    this.#applied = v;
    this.#level?.setTitle(levelText(v.level));
    this.#gauge.set(v.hasCollector ? Math.max(0, v.battery) : 0, v.batteryMax);
    this.#gauge.setVisible(v.hasCollector);
    const gaugeRect = this.getWndCtrl(WIDC_GAUGE)?.rect;
    this.#texts.setItems(
      v.hasCollector && gaugeRect
        ? [
            {
              x: gaugeRect.l + 5,
              y: gaugeRect.t,
              text: batteryText(v.battery, v.batteryMax),
              color: GAUGE_TEXT_COLOR,
            },
          ]
        : [],
    );
    // `SetButtonCaption( bIsStart )`: Stop while it runs
    if (v.collecting !== this.#started || this.#button?.state.get().texture === null) {
      this.#started = v.collecting;
      this.#button?.setTexture(v.collecting ? 'Buttstop.bmp' : 'ButtStart.bmp');
    }
  }

  // ---- input -------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.#deps.ports.actions.toggle();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `OnMouseWndSurface`: the battery and the level icon have a tooltip. */
  override onMouseMove(pt: Point): void {
    const o = this.getScreenClientRect();
    const tip = (r: Rect | null, tid: number): boolean => {
      if (!r || !rectContains(r, pt)) return false;
      this.#deps.tooltip.put(this, this.#deps.getText(tid), rectOffset(r, o.l, o.t), 0);
      return true;
    };
    if (tip(this.#batteryRect, TID_GAME_COLLECTOR_BETTERY_INFO)) return;
    tip(this.#levelRect, TID_GAME_COLLECTOR_LEVEL_INFO);
  }
}

export { EMPTY_COLLECTING_VIEW };
