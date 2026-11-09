// `SlotWindow` (the common shape of `CWndRemoveAttribute`, `CWndAwakening`, `CWndBlessingCancel`, `CWndLvReqDown`, `CWndPetAwakCancel` and
// `CWndRemovePiercing`; `src:_Interface/WndField.cpp:22450-23010`, `WndAwakening.cpp`, `WndBlessingCancel.cpp`, `WndLvReqDown.cpp`,
// `WndPetSys.cpp`; ref/22 §17 L, Plan/45 §18.1): a window with ONE 32 x 32 item slot (a layout static), a description text and a Start button.
//   * only an inventory item is dropped on the slot (any other source gets the forbid cursor); a worn item is refused with the forbid cursor;
//     the request decides whether the item fits and says why not; a new item replaces the old one (its reservation goes);
//   * the item is reserved (`SetExtra + 1`) while it sits in the slot and released when it leaves it or the window closes;
//   * a double click on the slot gives the item back and disables Start; Start is enabled while an item is in the slot;
//   * the description of the help panel fills the text box once.
// A subclass says which controls are the slot, the Start button and the text, and what Start does.

import { APP_INVENTORY } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PaperDoll } from '../char/PaperDoll';
import type { DialogItem } from '../dialogs/ports';
import type { SlotRequest } from './ports';

export const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };

export interface SlotWindowOptions {
  /** The `WIDC_*` id of the slot static. */
  readonly slotId: number;
  /** The `WIDC_*` id of the Start button. */
  readonly startId: number;
  /** The `WIDC_*` id of the description text, `null` = none. */
  readonly textId: number | null;
  /** The text of the description panel. */
  readonly description: string;
  /** Item tooltips over the slot (`PutToolTip_Item`); the windows of the original draw none. */
  readonly tooltip?: TooltipService;
  /** The `flag` of the tooltip (the applet id). */
  readonly tooltipFlag?: number;
}

export abstract class SlotWindow<R extends SlotRequest = SlotRequest> extends WndNeuz {
  protected readonly request: R;
  readonly #opts: SlotWindowOptions;
  #item: DialogItem | null = null;
  #slot: Rect = NO_RECT;
  #icon: PaperDoll | null = null;
  #released = false;

  protected constructor(request: R, opts: SlotWindowOptions) {
    super();
    this.request = request;
    this.#opts = opts;
    this.putRegInfo = false;
  }

  /** The item in the slot, `null` = empty. */
  get item(): DialogItem | null {
    return this.#item;
  }
  get slotRect(): Rect {
    return this.#slot;
  }
  get iconLayer(): PaperDoll | null {
    return this.#icon;
  }
  get startButton(): Button | null {
    return this.getDlgItem<Button>(this.#opts.startId);
  }
  get descriptionText(): TextCtrl | null {
    return this.#opts.textId === null ? null : this.getDlgItem<TextCtrl>(this.#opts.textId);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#slot = this.getWndCtrl(this.#opts.slotId)?.rect ?? NO_RECT;
    this.#icon = this.addControl(new PaperDoll(), 0x7e51, this.getClientRect(false));
    this.descriptionText?.setString(this.#opts.description);
    this.startButton?.enableWindow(false);
    this.moveParentCenter();
  }

  // ---- the slot -----------------------------------------------------------------------------------------------------------------

  /**
   * Puts an inventory item into the slot (`SetItem` / `SetWeapon` of the originals; the inventory's double click feeds the open window with
   * it). Returns `false` when the item is gone or the request refuses it.
   */
  setItem(objId: number): boolean {
    const item = this.request.item(objId);
    if (item === null || !this.request.accept(objId)) return false;
    if (this.#item) this.request.reserve(this.#item.objId, false);
    this.request.reserve(item.objId, true);
    this.#item = item;
    this.#refresh();
    return true;
  }

  /** Gives the item back (a double click on the slot). */
  clearItem(): void {
    if (this.#item === null) return;
    this.request.reserve(this.#item.objId, false);
    this.#item = null;
    this.#refresh();
  }

  /** `OnDropIcon`: an inventory item on the slot. */
  override onDropIcon(sc: Shortcut, ptWindow: Point): boolean {
    if (sc.kind !== SC.ITEM || (sc.from?.getFrameWnd()?.id ?? 0) !== APP_INVENTORY) {
      this.env.wm.drag.forbid();
      return false;
    }
    const pt = this.clientPoint(ptWindow);
    if (!rectContains(this.#slot, pt)) return true;
    return this.setItem(sc.id);
  }

  /** `OnLButtonDblClk` on the slot. */
  override onLButtonDblClk(pt: Point): void {
    if (rectContains(this.#slot, pt)) this.clearItem();
  }

  override onMouseMove(pt: Point): void {
    const tooltip = this.#opts.tooltip;
    if (!tooltip || this.#item === null || !rectContains(this.#slot, pt)) return;
    if (this.env.wm.isDraggingShortcut()) return;
    const s = this.getScreenClientRect();
    tooltip.putItem(
      this,
      this.#item,
      rectOffset(this.#slot, s.l, s.t),
      this.#opts.tooltipFlag ?? this.id,
    );
  }

  /** A window point (what `onDropIcon` is given) as a client point. */
  protected clientPoint(ptWindow: Point): Point {
    return {
      x: ptWindow.x - (this.rectClient.l - this.rectWindow.l),
      y: ptWindow.y - (this.rectClient.t - this.rectWindow.t),
    };
  }

  // ---- Start --------------------------------------------------------------------------------------------------------------------

  /** Start with an item in the slot. */
  protected abstract onStart(item: DialogItem): void;

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === this.#opts.startId) {
      if (this.#item !== null) this.onStart(this.#item);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  // ---- closing ------------------------------------------------------------------------------------------------------------------

  /** `OnDestroy`: the item is released. */
  override onDestroy(): void {
    this.release();
    this.request.closed?.();
    super.onDestroy();
  }

  /** Releases the reservation once (a window can end by Start before the destructor runs). */
  protected release(): void {
    if (this.#released) return;
    this.#released = true;
    if (this.#item) this.request.reserve(this.#item.objId, false);
  }

  #refresh(): void {
    const it = this.#item;
    this.startButton?.enableWindow(it !== null);
    this.#icon?.setLayers(
      it
        ? [
            {
              key: 'slot',
              x: this.#slot.l,
              y: this.#slot.t,
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
