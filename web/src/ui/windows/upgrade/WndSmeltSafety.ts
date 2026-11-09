// `WndSmeltSafety` (`Kia::CWndNewUpgrade`, `src:_Interface/WndNewUpgrade.cpp`, `__BETTER_SAFE_UPGRADES`, and the vanilla `CWndSmeltSafety`,
// `WndField.cpp:24202-25830`, `__SMELT_SAFETY`; ref/22 §17 L, Plan/45 §18.2; WP-UI-29 / WP-CFEAT-24): `APP_SMELT_SAFETY` (320 x 160), the safe
// upgrade window. ONE shell serves the auto-upgrade window (`features.better_safe_upgrades`) and the one-shot windows of the NPC menus (accessory,
// piercing, element, and the general one without the feature): they share the layout, and the state machine behind the port (`features/upgrade/safeSmelt`)
// decides what a drop, a click and Start do. The shell draws what the original draws and forwards the pointer:
//   * the target (`WIDC_STATIC1`), the material (`WIDC_STATIC11`), the protection scroll (`WIDC_STATIC41`) and the smelting scroll (`WIDC_STATIC61`)
//     as 32 x 32 icons at the top-left of their rects with the number of units left under the three material slots (title font, white, at
//     (left, bottom - 14)); the enchant glow on the target while the upgrade runs; the item tooltip over the target (a plain text over an empty target in
//     the one-shot window);
//   * the result bar `WIDC_STATIC31` (168 x 32): `SafetySuccess.bmp` / `SafetyFailure.bmp` after a try, the wait gauge `SafetyGauge.bmp` filling from the left
//     over `gameplay.enchant_time_s` while it runs (both one pixel right of the rect);
//   * `WIDC_NOW_GRADE` (the level, element level or sockets), the read-only edit `WIDC_EDIT_MAX_GRADE` with `-` / `+` (`WIDC_BUTTON_MINUS` / `_PLUS`), the captions
//     `WIDC_TITLE_NOW_GRADE` / `_MAX_GRADE`, the title (the tries line while running) and Start / Stop / Reset enabled by the view.
// Only inventory items are dropped (else the forbid cursor) and nothing while the upgrade runs; a click on a slot empties it (the machine ignores the click
// while it runs). FIX: the original printed the MATERIAL count under the scroll icons too (a missing brace in `OnDraw`); the shell prints each slot's own units.

import {
  APP_INVENTORY,
  APP_SMELT_SAFETY,
  WIDC_BUTTON_MINUS,
  WIDC_BUTTON_PLUS,
  WIDC_BUTTON_RESET,
  WIDC_BUTTON_START,
  WIDC_BUTTON_STOP,
  WIDC_EDIT_MAX_GRADE,
  WIDC_NOW_GRADE,
  WIDC_STATIC1,
  WIDC_STATIC11,
  WIDC_STATIC31,
  WIDC_STATIC41,
  WIDC_STATIC61,
  WIDC_TITLE_MAX_GRADE,
  WIDC_TITLE_NOW_GRADE,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import { Gauge } from '../../controls/Gauge';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { EBS_READONLY, Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll, type PaperLayer } from '../char/PaperDoll';
import { EnchantGlow } from './EnchantGlow';
import { SAFETY_FAILURE, SAFETY_GAUGE, SAFETY_SUCCESS } from './names';
import type { SmeltSafetyRequest, SmeltSafetyView, SmeltSlot } from './ports';
import { NO_RECT } from './SlotWindow';

export interface SmeltSafetyDeps {
  readonly request: SmeltSafetyRequest;
  readonly tooltip: TooltipService;
}

/** `nExtensionPixel`: the result bar is drawn one pixel right of its static. */
const EXTENSION_PIXEL = 1;
/** The gauge resolution handed to `Gauge.set` (fractions of a thousand). */
const GAUGE_STEPS = 1000;
const COUNT_COLOR = 0xffffffff;
/** `TRANSLUCENT_ALPHA` (75 of 255) of the hint icons of the vanilla window. */
const GHOST_ALPHA = 75 / 255;

export class WndSmeltSafety extends WndNeuz {
  readonly appId = APP_SMELT_SAFETY;
  override readonly processEveryFrame = true;
  readonly #deps: SmeltSafetyDeps;
  #view: SmeltSafetyView | null = null;
  #slots: Record<SmeltSlot, Rect> = {
    target: NO_RECT,
    material: NO_RECT,
    protection: NO_RECT,
    scroll: NO_RECT,
  };
  #bar: Rect = NO_RECT;
  #icons: PaperDoll | null = null;
  #counts: TextLayer | null = null;
  #sprites: SpriteLayer | null = null;
  #gauge: Gauge | null = null;
  #glow: EnchantGlow | null = null;
  #title = '';

  constructor(deps: SmeltSafetyDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** The last view the window applied. */
  get lastView(): SmeltSafetyView | null {
    return this.#view;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icons;
  }
  get countLayer(): TextLayer | null {
    return this.#counts;
  }
  get resultLayer(): SpriteLayer | null {
    return this.#sprites;
  }
  get gaugeBar(): Gauge | null {
    return this.#gauge;
  }
  get glowLayer(): EnchantGlow | null {
    return this.#glow;
  }
  get slotRects(): Readonly<Record<SmeltSlot, Rect>> {
    return this.#slots;
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_START);
  }
  get stopButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_STOP);
  }
  get resetButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_RESET);
  }
  get plusButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_PLUS);
  }
  get minusButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON_MINUS);
  }
  get maxEdit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT_MAX_GRADE);
  }
  get nowStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_NOW_GRADE);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const rect = (id: number): Rect => this.getWndCtrl(id)?.rect ?? NO_RECT;
    this.#slots = {
      target: rect(WIDC_STATIC1),
      material: rect(WIDC_STATIC11),
      protection: rect(WIDC_STATIC41),
      scroll: rect(WIDC_STATIC61),
    };
    this.#bar = rect(WIDC_STATIC31);
    this.maxEdit?.addWndStyle(EBS_READONLY);
    const client = this.getClientRect(false);
    this.#icons = this.addControl(new PaperDoll(), 0x7e61, client);
    this.#sprites = this.addControl(new SpriteLayer(), 0x7e62, client);
    this.#gauge = this.addControl(
      new Gauge({ fill: SAFETY_GAUGE }),
      0x7e63,
      rectOffset(this.#bar, EXTENSION_PIXEL, 0),
    );
    this.#glow = this.addControl(new EnchantGlow(), 0x7e64, client);
    this.#counts = this.addControl(new TextLayer(), 0x7e65, client);
    this.moveParentCenter();
    this.apply(this.#deps.request.view.get());
  }

  /** Every frame: the view is read and applied with diffing setters (an unchanged view costs no React commit). */
  override process(now: number): void {
    super.process(now);
    this.apply(this.#deps.request.view.get());
  }

  /** Applies a view (also called by the tests). */
  apply(v: SmeltSafetyView): void {
    this.#view = v;
    if (v.title !== this.#title) {
      this.#title = v.title;
      this.setTitle(v.title);
    }
    this.getDlgItem<Static>(WIDC_TITLE_NOW_GRADE)?.setTitle(v.nowCaption);
    this.getDlgItem<Static>(WIDC_TITLE_MAX_GRADE)?.setTitle(v.maxCaption);
    this.nowStatic?.setTitle(v.nowValue);
    this.maxEdit?.setString(v.maxValue);
    this.startButton?.enableWindow(v.canStart);
    this.stopButton?.enableWindow(v.canStop);
    this.resetButton?.enableWindow(v.canReset);
    this.plusButton?.enableWindow(v.canAdjust);
    this.minusButton?.enableWindow(v.canAdjust);

    // the icons at the top-left of their rects
    const layers: PaperLayer[] = [];
    const icon = (key: SmeltSlot, path: string | null, alpha = 1): void => {
      const r = this.#slots[key];
      layers.push({ key, x: r.l, y: r.t, size: 32, icon: path, alpha, tint: false });
    };
    if (v.target) icon('target', v.target.icon);
    if (v.material) icon('material', v.material.icon, v.material.ghost === true ? GHOST_ALPHA : 1);
    if (v.protection) {
      icon('protection', v.protection.icon, v.protection.ghost === true ? GHOST_ALPHA : 1);
    }
    if (v.scroll) icon('scroll', v.scroll.icon, v.scroll.ghost === true ? GHOST_ALPHA : 1);
    this.#icons?.setLayers(layers, true);

    // the units left under the slots (`TextOut( rect.left, rect.bottom - 14 )` in the title font)
    const counts: TextItem[] = [];
    const count = (key: SmeltSlot, n: number | undefined): void => {
      if (n === undefined || n <= 0) return;
      const r = this.#slots[key];
      counts.push({ x: r.l, y: r.b - 14, text: String(n), color: COUNT_COLOR, font: 'title' });
    };
    count('material', v.material?.count);
    count('protection', v.protection?.count);
    count('scroll', v.scroll?.count);
    this.#counts?.setItems(counts);

    // the result image and the wait gauge share the bar
    const sprites: SpriteEntry[] = [];
    if (v.result !== 0) {
      sprites.push({
        key: 'result',
        path: v.result === 1 ? SAFETY_SUCCESS : SAFETY_FAILURE,
        x: this.#bar.l + EXTENSION_PIXEL,
        y: this.#bar.t,
        w: this.#bar.r - this.#bar.l,
        h: this.#bar.b - this.#bar.t,
      });
    }
    this.#sprites?.setEntries(sprites);
    this.#gauge?.set(v.running ? Math.round(v.gauge * GAUGE_STEPS) : 0, GAUGE_STEPS);
    const t = this.#slots.target;
    this.#glow?.setSpots(v.running && v.target ? [{ x: t.l + 2, y: t.t }] : []);
  }

  // ---- the pointer --------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an inventory item anywhere on the client; nothing while the upgrade runs. */
  override onDropIcon(sc: Shortcut): boolean {
    if (this.#view?.running === true) return false;
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    this.#deps.request.actions.drop(sc.id);
    return true;
  }

  /** `OnLButtonUp`: a click on a slot empties it. */
  override onLButtonUp(pt: Point): void {
    for (const key of ['target', 'material', 'protection', 'scroll'] as const) {
      if (rectContains(this.#slots[key], pt)) {
        this.#deps.request.actions.clear(key);
        return;
      }
    }
  }

  /** The item tooltip over the target, or the plain text over an empty target of the one-shot window. */
  override onMouseMove(pt: Point): void {
    const v = this.#view;
    const t = this.#slots.target;
    if (!v || !rectContains(t, pt) || this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    const at = rectOffset(t, s.l, s.t);
    if (v.target) this.#deps.tooltip.putItem(this, v.target, at, APP_SMELT_SAFETY);
    else if (v.emptyTip !== null) this.#deps.tooltip.put(this, v.emptyTip, at, 1);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      const a = this.#deps.request.actions;
      switch (id) {
        case WIDC_BUTTON_START:
          a.start();
          return true;
        case WIDC_BUTTON_STOP:
          a.stop();
          return true;
        case WIDC_BUTTON_RESET:
          a.reset();
          return true;
        case WIDC_BUTTON_PLUS:
          a.adjust(1);
          return true;
        case WIDC_BUTTON_MINUS:
          a.adjust(-1);
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `~CWndNewUpgrade`: every reservation is released. */
  override onDestroy(): void {
    this.#deps.request.actions.closed();
    super.onDestroy();
  }
}
