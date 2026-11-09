// `WndRandomOptionRetry` (`CWndRandomOptionRetry`, `src:_Interface/WndField.cpp:26254-26510`, `__RANDOMOPTION_RETRY`, `features.randomoption_retry`,
// ref/22 §17 L, Plan/42 §19.4 L, 45 §18.5; WP-UI-29 / WP-CFEAT-15): `APP_RANDOMOPTIONRETRY` (255 x 160), the scroll of awakening augmentation
// (`II_SYS_SYS_SCR_RANDOMOPTIONRTRY`, rule D6). An awakened item is dropped on the picture (`WIDC_PICTURE1`, the icon at +6, +6, the item
// tooltip over it); the item needs MORE THAN ONE awakening line (`GetRandomOptionSize` > 1 and the awakening kind), else a message box says so.
// The radio buttons `WIDC_RADIO1..3` list the lines (two enabled for two lines, all three for three; their titles are the lines, read again
// whenever the item changes); a checked radio enables Start, which sends `RandomOptionRetry{item, scroll, line 1..3}` and leaves the window open
// (the server answers with the new item). The scroll is reserved while the window is open, the item while it sits in the slot. A right click on the
// picture takes the item out again (radios disabled and unchecked, the titles back to "Awake Option N"); Close ends the window.

import {
  APP_INVENTORY,
  APP_RANDOMOPTIONRETRY,
  WIDC_CANCEL,
  WIDC_PICTURE1,
  WIDC_RADIO1,
  WIDC_RADIO2,
  WIDC_RADIO3,
  WIDC_START,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { DialogItem } from '../dialogs/ports';
import type { RandomOptionRetryRequest } from './ports';

export interface RandomOptionRetryDeps {
  readonly request: RandomOptionRetryRequest;
  readonly tooltip: TooltipService;
}

const RADIO_IDS = [WIDC_RADIO1, WIDC_RADIO2, WIDC_RADIO3] as const;

export class WndRandomOptionRetry extends WndNeuz {
  readonly appId = APP_RANDOMOPTIONRETRY;
  readonly #deps: RandomOptionRetryDeps;
  #item: DialogItem | null = null;
  #picture: Rect = { l: 0, t: 0, r: 0, b: 0 };
  #icon: PaperDoll | null = null;
  #off: (() => void) | null = null;
  #released = false;

  constructor(deps: RandomOptionRetryDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get item(): DialogItem | null {
    return this.#item;
  }
  get pictureRect(): Rect {
    return this.#picture;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icon;
  }
  get radios(): readonly (Button | null)[] {
    return RADIO_IDS.map((id) => this.getDlgItem<Button>(id));
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_START);
  }
  get closeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }
  /** The checked radio, 1..3, `0` = none (`nRadio`). */
  get line(): number {
    const i = this.radios.findIndex((r) => (r?.getCheck() ?? 0) !== 0);
    return i + 1;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#picture = this.getWndCtrl(WIDC_PICTURE1)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
    this.#icon = this.addControl(new PaperDoll(), 0x7e11, this.getClientRect(false));
    // `SetItemId`: the scroll is reserved while the window is open
    this.#deps.request.reserve(this.#deps.request.scroll.objId, true);
    this.#off = this.#deps.request.subscribe(() => {
      this.#syncLines();
    });
    this.#clear();
    this.moveParentCenter();
  }

  // ---- the slot -----------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an awakened item with more than one line onto the picture (a worn item is refused with the forbid cursor). */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    const req = this.#deps.request;
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) return false;
    if (req.worn(sc.id)) {
      req.sayWorn();
      this.env.wm.drag.forbid();
      return false;
    }
    const item = req.item(sc.id);
    if (!item) return false;
    const pt: Point = {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
    if (!rectContains(this.#picture, pt)) return true;
    const lines = req.lines(item.objId);
    if (lines === null || lines.length < 2) {
      req.refusedItem();
      return true;
    }
    if (this.#item) req.reserve(this.#item.objId, false);
    req.reserve(item.objId, true);
    this.#item = item;
    this.#syncLines();
    return true;
  }

  /** `OnRButtonUp` on the picture: the item goes back and everything is disabled. */
  override onRButtonUp(pt: Point): void {
    if (!this.#item || !rectContains(this.#picture, pt)) return;
    this.#deps.request.reserve(this.#item.objId, false);
    this.#item = null;
    this.#clear();
  }

  /** `OnDraw`: the radio titles are the lines of the item as it is now, and Start needs a checked radio. */
  #syncLines(): void {
    const req = this.#deps.request;
    const item = this.#item;
    if (!item) {
      this.#paint();
      return;
    }
    const lines = req.lines(item.objId) ?? [];
    RADIO_IDS.forEach((id, i) => {
      const radio = this.getDlgItem<Button>(id);
      if (!radio) return;
      const text = lines[i];
      radio.enableWindow(text !== undefined);
      if (text !== undefined) {
        radio.setTitle(text);
      } else {
        radio.setCheck(false);
        radio.setTitle(this.#deps.request.defaults[i] ?? '');
      }
    });
    this.#syncStart();
    this.#paint();
  }

  #syncStart(): void {
    const anyChecked = this.radios.some((r) => (r?.getCheck() ?? 0) !== 0 && r?.isWindowEnabled());
    this.startButton?.enableWindow(this.#item !== null && anyChecked);
  }

  /** The empty state: radios disabled, unchecked and titled "Awake Option N"; Start disabled. */
  #clear(): void {
    RADIO_IDS.forEach((id, i) => {
      const radio = this.getDlgItem<Button>(id);
      if (!radio) return;
      radio.enableWindow(false);
      radio.setCheck(false);
      radio.setTitle(this.#deps.request.defaults[i] ?? '');
    });
    this.startButton?.enableWindow(false);
    this.#paint();
  }

  // ---- buttons ------------------------------------------------------------------------------------------------------------------

  /** Start: sends the retry for the checked line; the window stays open. */
  start(): void {
    const item = this.#item;
    const line = this.line;
    if (!item || line < 1) return;
    this.#deps.request.send(item.objId, this.#deps.request.scroll.objId, line);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_START) {
        this.start();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
      if (RADIO_IDS.some((r) => r === id)) {
        const handled = super.onChildNotify(msg, id, src);
        this.#syncStart();
        return handled;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `OnDraw`: Start is enabled while a radio is checked (the radio group checks the clicked radio AFTER the parent was notified). */
  override process(now: number): void {
    if (this.#item !== null) this.#syncStart();
    super.process(now);
  }

  /** The item tooltip over the picture (`OnDraw`). */
  override onMouseMove(pt: Point): void {
    const item = this.#item;
    if (!item || !rectContains(this.#picture, pt) || this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.putItem(
      this,
      item,
      rectOffset(this.#picture, s.l, s.t),
      APP_RANDOMOPTIONRETRY,
    );
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    if (!this.#released) {
      this.#released = true;
      const req = this.#deps.request;
      if (this.#item) req.reserve(this.#item.objId, false);
      req.reserve(req.scroll.objId, false);
    }
    super.onDestroy();
  }

  #paint(): void {
    const it = this.#item;
    this.#icon?.setLayers(
      it
        ? [
            {
              key: 'item',
              x: this.#picture.l + 6,
              y: this.#picture.t + 6,
              size: 32,
              icon: it.icon,
              alpha: 1,
              tint: false,
            },
          ]
        : [],
      true,
    );
  }
}
