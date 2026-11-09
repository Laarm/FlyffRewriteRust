// `WndBuffStatus` (`CWndBuffStatus`, `src:_Interface/WndField.cpp:18968-19700`, ref/22 §17 C, Plan/42 §19.4 D, §19.5, 45 §12; WP-UI-28):
// `APP_BUFF_STATUS`, the skill-buff window the world opens while the player has any skill buff. No title bar and no frame. It opens at
// the TOP LEFT, directly right of the character status window (`APP_STATUS1`), and right of a pet status window that sits there, never
// overlapping them (`buffStatusDefaultPos`; owner #004 - the original opened it at (width / 3, height / 3)); below the status window when
// the layout is too narrow. Like the original (`m_bPutRegInfo` stays TRUE) it REMEMBERS where the player dragged it: the rect is saved
// whenever the window is destroyed (the last buff ends, the logout) and restored at the next opening and the next login; the default
// above applies only when nothing is stored. 28 slots (`MAX_SKILLBUFF_COUNT`) of a 34 px pitch: mode 0 is seven
// columns (238 x 136), mode 1 seven rows (136 x 238); a double click on an icon (the right double click of the original is not routed by the window manager) switches the mode and stores it
// (`g_Option.m_BuffStatusMode`). Only the icons claim the pointer (34 x 34, `GetHitTestResult`): everything else passes to the world.
// The claim is real: the window manager's press, move and wheel over an icon are marked for the world input (`wm/pointerClaims.ts`), so
// an icon never walks the character, selects a monster or zooms the camera (the DOM element itself is `pointer-events: none`).
// The tooltip rect of an icon is 28 x 28; the Esc chain skips the window (`ESC_SKIP_APPS`).

import {
  APP_BUFF_STATUS,
  APP_BUFFPET_STATUS,
  APP_PET_STATUS,
  APP_STATUS1,
} from '../../../gen/defs/resdata';
import { IconStrip } from '../../controls/IconStrip';
import {
  type Point,
  type Rect,
  rectContains,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../../layout/geometry';
import { WBS_CAPTION, WBS_NODRAWFRAME, WBS_NOFOCUS } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { BUFF_TIP_SIZE, buffEntry } from './buffEntries';
import type { BuffActions, BuffStatusView } from './ports';

/** `MAX_SKILLBUFF_COUNT`. */
export const MAX_SKILLBUFF_COUNT = 28;
/** The pitch of the slots. */
export const BUFF_SLOT = 34;
/** Window sizes of the two modes. */
export const BUFF_STATUS_SIZE = { 0: { w: 238, h: 136 }, 1: { w: 136, h: 238 } } as const;

/** The size of `APP_STATUS1` in the layout (`resdata`, checked against the real bundle): the anchor of the default place when the status window is closed. */
export const STATUS1_SIZE = { w: 220, h: 112 } as const;

/**
 * Where the skill-buff window opens when nothing is stored (owner #004): directly right of the status window (`status`, or the place
 * it would have at the top left of `layout` when it is not open), then right of every rect of `occupied` it would overlap (the pet
 * status windows, which also place themselves at the status window's right edge). When it does not fit in the layout width it goes
 * below the status window instead. Clamped into the layout. It never overlaps the status window.
 */
export function buffStatusDefaultPos(
  layout: Rect,
  status: Rect | null,
  size: { readonly w: number; readonly h: number },
  occupied: readonly Rect[],
): Point {
  const anchor: Rect = status ?? {
    l: layout.l,
    t: layout.t,
    r: layout.l + STATUS1_SIZE.w,
    b: layout.t + STATUS1_SIZE.h,
  };
  let x = anchor.r;
  let y = anchor.t;
  const overlaps = (r: Rect): boolean => x < r.r && x + size.w > r.l && y < r.b && y + size.h > r.t;
  for (let pass = 0; pass <= occupied.length; pass += 1) {
    const hit = occupied.find(overlaps);
    if (!hit) break;
    x = hit.r;
  }
  if (x + size.w > layout.r) {
    x = anchor.l;
    y = anchor.b;
  }
  return {
    x: Math.max(layout.l, Math.min(x, layout.r - size.w)),
    y: Math.max(layout.t, Math.min(y, layout.b - size.h)),
  };
}

/** The top-left of slot `i` in a mode (`SetBuffIconInfo`). */
export function buffSlotPoint(mode: 0 | 1, i: number): Point {
  const row = Math.floor(i / 7);
  const col = i % 7;
  return mode === 0
    ? { x: col * BUFF_SLOT, y: row * BUFF_SLOT }
    : { x: row * BUFF_SLOT, y: col * BUFF_SLOT };
}

export interface BuffStatusDeps {
  readonly actions: BuffActions;
  readonly tooltip: TooltipService;
}

export class WndBuffStatus extends WndNeuz {
  readonly appId = APP_BUFF_STATUS;
  readonly #deps: BuffStatusDeps;
  #mode: 0 | 1;
  #strip: IconStrip | null = null;
  #view: BuffStatusView = { icons: [] };
  #applied: BuffStatusView | null = null;
  #hover = -1;

  constructor(deps: BuffStatusDeps) {
    super();
    this.#deps = deps;
    this.#mode = deps.actions.mode();
    // `putRegInfo` stays true (the original's `m_bPutRegInfo = TRUE`): the dragged place is remembered
    // only the icons claim the pointer (`isPickupSpace`): the rest of the 238 x 136 box belongs to the world canvas below
    this.domPassthrough = true;
  }

  /**
   * `InitDialog(APP_BUFF_STATUS, WBS_NOFOCUS)`; `PaintFrame` is empty in the original (the window draws only its icons), so the
   * skin of the layout is not baked (`WBS_NODRAWFRAME`).
   */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_NOFOCUS | WBS_NODRAWFRAME | extraStyle);
  }

  get mode(): 0 | 1 {
    return this.#mode;
  }
  get strip(): IconStrip | null {
    return this.#strip;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.delWndStyle(WBS_CAPTION);
    this.titleBar?.setVisible(false);
    this.#strip = this.addControl(new IconStrip(), 0x7c10, { l: 0, t: 0, r: 238, b: 238 });
    this.#layout();
    // the default place (a stored rect replaces it after `initialize`, `WindowManager.open`)
    const wm = this.env.wm;
    const status = wm.get(APP_STATUS1);
    const occupied = [APP_PET_STATUS, APP_BUFFPET_STATUS].flatMap((app) => {
      const w = wm.get(app);
      return w?.isVisible() === true ? [w.rectWindow] : [];
    });
    this.move(
      buffStatusDefaultPos(
        wm.layoutRect,
        status?.isVisible() === true ? status.rectWindow : null,
        // the size of the mode as the frame made it (`BUFF_STATUS_SIZE` plus the border of the frame)
        { w: rectWidth(this.rectWindow), h: rectHeight(this.rectWindow) },
        occupied,
      ),
    );
    this.#apply(this.#deps.actions.status.get());
  }

  /** `SetBuffIconInfo`: the window takes the size of the mode. */
  #layout(): void {
    const size = BUFF_STATUS_SIZE[this.#mode];
    const r = this.rectWindow;
    this.setWndRect({ l: r.l, t: r.t, r: r.l + size.w, b: r.t + size.h }, true);
    this.#strip?.setWndRect({ l: 0, t: 0, r: size.w, b: size.h }, true);
  }

  override process(now: number): void {
    const v = this.#deps.actions.status.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #apply(v: BuffStatusView): void {
    this.#applied = v;
    this.#view = v;
    this.#paint();
  }

  #paint(): void {
    const entries = this.#view.icons.slice(0, MAX_SKILLBUFF_COUNT).flatMap((icon, i) => {
      const p = buffSlotPoint(this.#mode, i);
      return buffEntry(icon, p.x, p.y);
    });
    this.#strip?.setEntries(entries);
  }

  /** The slot index under a client point (`34 x 34` of an existing icon), `-1` for none. */
  hitSlot(pt: Point): number {
    const n = Math.min(this.#view.icons.length, MAX_SKILLBUFF_COUNT);
    for (let i = 0; i < n; i += 1) {
      const p = buffSlotPoint(this.#mode, i);
      if (rectContains({ l: p.x, t: p.y, r: p.x + BUFF_SLOT, b: p.y + BUFF_SLOT }, pt)) return i;
    }
    return -1;
  }

  /** `GetHitTestResult`: only the icons claim the pointer; an empty part of the window belongs to the world. */
  override isPickupSpace(ptWindow: Point): boolean {
    const co = {
      x: this.rectClient.l - this.rectWindow.l,
      y: this.rectClient.t - this.rectWindow.t,
    };
    return this.hitSlot({ x: ptWindow.x - co.x, y: ptWindow.y - co.y }) >= 0;
  }

  #toggle(): void {
    this.#mode = this.#mode === 0 ? 1 : 0;
    this.#deps.actions.setMode(this.#mode);
    this.#layout();
    this.#paint();
  }

  override onLButtonDblClk(pt: Point): void {
    if (this.hitSlot(pt) >= 0) this.#toggle();
  }

  /** The tooltip of the icon under the pointer (28 x 28). */
  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const i = this.hitSlot(pt);
    const icon = i >= 0 ? this.#view.icons[i] : undefined;
    if (!icon) {
      if (this.#hover >= 0) {
        this.#hover = -1;
        this.#deps.tooltip.cancel();
      }
      return;
    }
    const p = buffSlotPoint(this.#mode, i);
    const runs = this.#deps.actions.tooltip(icon.key);
    if (!runs) return;
    this.#hover = i;
    const s = this.getScreenClientRect();
    const rect: Rect = rectOffset(
      { l: p.x, t: p.y, r: p.x + BUFF_TIP_SIZE, b: p.y + BUFF_TIP_SIZE },
      s.l,
      s.t,
    );
    this.#deps.tooltip.put(this, runs, rect, 0);
  }

  override onMouseLeave(): void {
    if (this.#hover >= 0) {
      this.#hover = -1;
      this.#deps.tooltip.cancel();
    }
  }
}
