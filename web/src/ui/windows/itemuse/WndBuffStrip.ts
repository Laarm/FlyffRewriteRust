// `WndBuffStrip` (the buff icons of `CWndWorld::OnDraw` / `RenderBuff`, `src:_Interface/WndWorld.cpp:9400-11020`, ref/22 §17 C,
// Plan/42 §19.5, 45 §12; WP-UI-28): the transparent window under every other window that carries what the original draws into the
// world view - the skill, party and item buff icons in two rows around the top centre, and the cast bar of a timed item. The
// positions, the wrap limit and the gap are the buff feature's (`BuffActions.hud(width, height)`, a pure function of the viewport
// width); the window paints them, shows the tooltip of the icon under the pointer, forwards a click (the angel buffs act on it) and a double
// click (the return scroll, the party summon and the feed pocket buffs act on it). Only the icons claim the pointer; everything else belongs to the world.
// The claim is real (BFIX1 #004): the window manager's press, move and wheel over an icon are marked for the world input
// (`wm/pointerClaims.ts`), so a click on an item, party, premium, angel or return-scroll icon never walks the character. The party lane flows
// around the open skill-buff window: `#refresh` passes its rect (strip client space) as `avoid` to `BuffActions.hud`.
//
// It has no layout row (`APP_*` ids are the layouts'): it is created by `ItemUseWindowsService.openBuffStrip` with a private app id.

import { APP_BUFF_STATUS } from '../../../gen/defs/resdata';
import { Gauge } from '../../controls/Gauge';
import { IconStrip } from '../../controls/IconStrip';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { WBS_MANAGER, WBS_NODRAWFRAME, WBS_NOFOCUS, WBS_NOFRAME } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { buffEntry } from './buffEntries';
import type { BuffActions, BuffHudView } from './ports';

/** The private id of the strip window (no `APP_*` layout). */
export const APP_BUFF_STRIP = 0x6b01;
/** `RenderCasting`: the bar is 200 px wide, white back (alpha 200) and red fill (alpha 128, (255, 15, 15)). */
export const CAST_WIDTH = 200;
export const CAST_HEIGHT = 12;
export const CAST_BACK = 0xc8ffffff;
export const CAST_FILL = 0x80ff0f0f;
const GAUGE = 'GauEmptyNormal.bmp';

export interface BuffStripDeps {
  readonly actions: BuffActions;
  readonly tooltip: TooltipService;
}

export class WndBuffStrip extends WndNeuz {
  readonly appId = APP_BUFF_STRIP;
  readonly #deps: BuffStripDeps;
  #strip: IconStrip | null = null;
  #castBack: Gauge | null = null;
  #castFill: Gauge | null = null;
  #view: BuffHudView = { icons: [], cast: null };
  #hover = '';

  constructor(deps: BuffStripDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
    // the window covers the whole view: its element must not take the pointer events of the world canvas below
    this.domPassthrough = true;
  }

  /** The window covers the layout rect and sits in the manager layer, below every window, without taking the focus. */
  override initialize(parent: WndBase): boolean {
    const l = parent.env.wm.layoutRect;
    return this.create(
      WBS_MANAGER | WBS_NOFRAME | WBS_NODRAWFRAME | WBS_NOFOCUS,
      { l: l.l, t: l.t, r: l.r, b: l.b },
      parent,
      APP_BUFF_STRIP,
    );
  }

  get strip(): IconStrip | null {
    return this.#strip;
  }
  get cast(): { back: Gauge; fill: Gauge } | null {
    return this.#castBack && this.#castFill ? { back: this.#castBack, fill: this.#castFill } : null;
  }
  get hud(): BuffHudView {
    return this.#view;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const all = {
      l: 0,
      t: 0,
      r: this.rectWindow.r - this.rectWindow.l,
      b: this.rectWindow.b - this.rectWindow.t,
    };
    this.#strip = this.addControl(new IconStrip(), 0x6b10, all);
    this.#castBack = this.addControl(new Gauge({ fill: GAUGE, color: CAST_BACK }), 0x6b11, {
      l: 0,
      t: 0,
      r: CAST_WIDTH,
      b: CAST_HEIGHT,
    });
    this.#castFill = this.addControl(new Gauge({ fill: GAUGE, color: CAST_FILL }), 0x6b12, {
      l: 0,
      t: 0,
      r: CAST_WIDTH,
      b: CAST_HEIGHT,
    });
    this.#castBack.setVisible(false);
    this.#castFill.setVisible(false);
    this.#refresh();
  }

  override process(now: number): void {
    this.#refresh();
    super.process(now);
  }

  #refresh(): void {
    const l = this.env.wm.layoutRect;
    const w = l.r - l.l;
    const h = l.b - l.t;
    const r = this.rectWindow;
    if (r.r - r.l !== w || r.b - r.t !== h || r.l !== l.l || r.t !== l.t) {
      this.setWndRect({ l: l.l, t: l.t, r: l.r, b: l.b }, true);
      this.#strip?.setWndRect({ l: 0, t: 0, r: w, b: h }, true);
    }
    // the party lane keeps clear of the open skill-buff window (it sits at the top left, over the lane's left end on narrow views)
    const buff = this.env.wm.get(APP_BUFF_STATUS);
    const sc = this.getScreenClientRect();
    const avoid: Rect[] =
      buff?.isVisible() === true ? [rectOffset(buff.rectWindow, -sc.l, -sc.t)] : [];
    const v = this.#deps.actions.hud(w, avoid);
    this.#view = v;
    this.#strip?.setEntries(v.icons.flatMap((i) => buffEntry(i, i.x, i.y)));
    const cast = v.cast;
    const x = Math.trunc((w - CAST_WIDTH) / 2);
    const y = Math.trunc(h / 2);
    for (const g of [this.#castBack, this.#castFill]) {
      if (g) {
        g.setVisible(cast !== null);
        g.setWndRect({ l: x, t: y, r: x + CAST_WIDTH, b: y + CAST_HEIGHT }, true);
      }
    }
    this.#castBack?.set(CAST_WIDTH, CAST_WIDTH);
    this.#castFill?.set(
      Math.trunc(CAST_WIDTH * Math.max(0, Math.min(1, cast?.fraction ?? 0))),
      CAST_WIDTH,
    );
  }

  /** The icon under a client point (its picture). */
  iconAt(pt: Point): string | null {
    for (const i of this.#view.icons) {
      if (rectContains({ l: i.x, t: i.y, r: i.x + i.size, b: i.y + i.size }, pt)) return i.key;
    }
    return null;
  }

  override isPickupSpace(ptWindow: Point): boolean {
    const co = {
      x: this.rectClient.l - this.rectWindow.l,
      y: this.rectClient.t - this.rectWindow.t,
    };
    return this.iconAt({ x: ptWindow.x - co.x, y: ptWindow.y - co.y }) !== null;
  }

  override onLButtonDown(pt: Point): void {
    const key = this.iconAt(pt);
    if (key !== null) this.#deps.actions.click?.(key);
  }

  override onLButtonDblClk(pt: Point): void {
    const key = this.iconAt(pt);
    if (key !== null) this.#deps.actions.activate(key);
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const key = this.iconAt(pt);
    if (key === null) {
      if (this.#hover !== '') {
        this.#hover = '';
        this.#deps.tooltip.cancel();
      }
      return;
    }
    const icon = this.#view.icons.find((i) => i.key === key);
    const runs = this.#deps.actions.tooltip(key);
    if (!icon || !runs) return;
    this.#hover = key;
    const s = this.getScreenClientRect();
    const rect = rectOffset(
      { l: icon.x, t: icon.y, r: icon.x + icon.tipSize, b: icon.y + icon.tipSize },
      s.l,
      s.t,
    );
    this.#deps.tooltip.put(this, runs, rect, 0);
  }

  override onMouseLeave(): void {
    if (this.#hover !== '') {
      this.#hover = '';
      this.#deps.tooltip.cancel();
    }
  }
}
