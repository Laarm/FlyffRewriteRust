// `WndVendor` (`CWndVendor`, `src:_Interface/WndVendor.cpp:1-370`, `WndVendorCtrl.cpp`, ref/22 §17 F, Plan/42 §19.4 F, 45 §15; WP-UI-26):
// `APP_VENDOR_REVISION` (448 x 464), the personal shop. ONE window for both roles:
//
//   * OWNER ("Private Shop"): the title edit (`WIDC_EDIT1`), OK (opens the shop), Cancel, Reset (unregisters everything; vanishes while the shop is open),
//     Chat, and the 20 slots (two columns of ten, `VendorGrid`). An inventory item dropped on a free slot of a CLOSED shop starts the price window; a
//     right click or dragging an item back to the bag unregisters it. Once the shop is open the title is locked and the goods cannot change.
//   * VISITOR: the title of the visited shop in the (disabled) edit, no OK / Cancel / Reset; a double click on an item, or dragging it to the bag, opens
//     the purchase window.
//
// The twenty edit boxes `WIDC_EDIT2-21` of the layout are only the panels behind the slots: they are disabled (the grid, created after them, takes the
// pointer). The window is not an Esc target (`ESC_SKIP_APPS`) and is not remembered. It opens beside the inventory (left of it when the inventory is on the
// right half of the screen).

import { ITYPE_ITEM } from '../../../gen/defs/define';
import {
  APP_INVENTORY,
  APP_VENDOR_REVISION,
  WIDC_CANCEL,
  WIDC_CHAT,
  WIDC_EDIT1,
  WIDC_OK,
  WIDC_RESET,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { type Point, type Rect, rectWidth } from '../../layout/geometry';
import { VendorGrid, type VendorCell } from '../../lists/VendorGrid';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { VendorActions, VendorEntry, VendorView, ViewSource } from './ports';

/** `m_wndctrlVendor.Create( WLVS_ICON | WBS_NODRAWFRAME, CRect( 5, 60, 440, 460 ), this, 100 )`. */
export const WIDC_VENDOR_GRID = 100;
export const VENDOR_GRID_RECT: Rect = { l: 5, t: 60, r: 440, b: 460 };
/** The longest title the edit takes (the protocol's `PVendorOpenReq.title`). */
export const VENDOR_TITLE_MAX = 47;
/** `WIDC_EDIT2`-`WIDC_EDIT21`: the panels behind the slots (window ids 897, 794, 436 ... 1185). */
const PANEL_NAMES: readonly string[] = Array.from(
  { length: 20 },
  (_, i) => `WIDC_EDIT${String(i + 2)}`,
);

export interface VendorDeps {
  readonly view: ViewSource<VendorView>;
  readonly actions: VendorActions;
  readonly tooltip: TooltipService;
  /** `Initialize` returns `FALSE` for a duel and for an open trade, shop, bank or guild bank window. */
  readonly canOpen?: (() => boolean) | undefined;
}

export class WndVendor extends WndNeuz {
  readonly appId = APP_VENDOR_REVISION;
  readonly #deps: VendorDeps;
  readonly #cells = new WeakMap<VendorEntry, VendorCell>();
  #grid: VendorGrid | null = null;
  #edit: EditCtrl | null = null;
  #applied: VendorView | null = null;
  #okHidden = false;
  #shownTitle: string | null = null;
  #off: (() => void) | null = null;
  #closedNotified = false;

  constructor(deps: VendorDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    if (this.#deps.canOpen && !this.#deps.canOpen()) return false;
    return super.initialize(parent, extraStyle);
  }

  // ---- accessors for tests and the feature layer -----------------------------------------------------------------------

  get grid(): VendorGrid | null {
    return this.#grid;
  }
  get titleEdit(): EditCtrl | null {
    return this.#edit;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }
  get resetButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_RESET);
  }
  get chatButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CHAT);
  }
  /** The text of the title edit. */
  get titleText(): string {
    return this.#edit?.getString() ?? '';
  }

  // ---- OnInitialUpdate -------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.setMaxLength(VENDOR_TITLE_MAX);
    for (const name of PANEL_NAMES) this.getDlgItemByName(name)?.enableWindow(false);
    this.#grid = this.addControl(
      new VendorGrid(
        {
          shortcutOf: (slot) => this.#payload(slot),
          hover: (slot, r) => {
            this.#hover(slot, r);
          },
          dblClick: (slot) => {
            if (!this.#deps.view.get().owner) this.#deps.actions.buy(slot);
          },
          rightClick: (slot) => {
            const v = this.#deps.view.get();
            if (v.owner && !v.open) this.#deps.actions.unregister(slot);
          },
          drop: (sc, slot) => this.#drop(sc, slot),
        },
        true,
        false,
      ),
      WIDC_VENDOR_GRID,
      VENDOR_GRID_RECT,
    );
    this.#placeNextToInventory();
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
    const v = this.#deps.view.get();
    if (v.owner && !v.open) this.#deps.actions.opened?.();
  }

  /** The window opens beside the inventory: left of it when the inventory is on the right half of the root, else right of it. */
  #placeNextToInventory(): void {
    const inv = this.env.wm.get(APP_INVENTORY);
    if (!inv) {
      this.moveParentCenter();
      return;
    }
    const mine = rectWidth(this.rectWindow);
    const r = inv.rectWindow;
    const root = this.env.wm.layoutRect;
    const pt: Point =
      r.l > (root.r - root.l) / 2 ? { x: r.l - mine, y: r.t } : { x: r.l + rectWidth(r), y: r.t };
    this.move(pt);
  }

  // ---- the view --------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #cellOf(e: VendorEntry): VendorCell {
    let c = this.#cells.get(e);
    if (!c) {
      c = {
        icon: e.icon,
        count: e.count,
        showCount: e.showCount,
        tint: e.expired ? 'red' : null,
        name: e.name,
        option: e.option,
        price: e.price,
      };
      this.#cells.set(e, c);
    }
    return c;
  }

  #apply(v: VendorView): void {
    this.#applied = v;
    if (this.getTitle() !== v.windowTitle) this.setTitle(v.windowTitle);
    const edit = this.#edit;
    if (edit) {
      // the owner types into the edit while the shop is closed; a visitor and an open shop show the title read-only. The feature's title goes
      // in when it changes (a visited shop, the title restored after the owner stopped selling); what the owner types is the edit's own.
      const locked = !v.owner || v.open;
      if (v.title !== this.#shownTitle || (locked && edit.getString() !== v.title)) {
        edit.setString(v.title);
        this.#shownTitle = v.title;
      }
      edit.enableWindow(!locked);
    }
    const setup = v.owner && !v.open;
    // OK: only to open the shop; hidden while its confirmation box is open (`restoreOk`)
    this.okButton?.setVisible(setup && !this.#okHidden);
    this.cancelButton?.setVisible(v.owner);
    // `CWndVendor::Process`: Reset is visible while the shop is closed (and for the owner only)
    this.resetButton?.setVisible(setup);
    const cells: (VendorCell | null)[] = [];
    for (let i = 0; i < 20; i += 1) {
      const e = v.cells[i];
      cells.push(e ? this.#cellOf(e) : null);
    }
    this.#grid?.setCells(cells);
  }

  /** The OK button comes back (the confirmation box was cancelled). */
  restoreOk(): void {
    this.#okHidden = false;
    this.#apply(this.#deps.view.get());
  }

  // ---- the grid --------------------------------------------------------------------------------------------------------------

  #payload(slot: number): Shortcut | null {
    const e = this.#deps.view.get().cells[slot];
    if (!e) return null;
    return {
      kind: SC.ITEM,
      id: e.objId,
      type: ITYPE_ITEM,
      index: slot,
      userId: 0,
      data: 1,
      itemId: e.itemId,
      ...(e.icon === null ? {} : { icon: e.icon }),
    };
  }

  #hover(slot: number | null, r: Rect | null): void {
    const v = this.#deps.view.get();
    const hover = itemHover({
      tooltip: this.#deps.tooltip,
      owner: this,
      flag: APP_VENDOR_REVISION,
      itemAt: (s) => v.cells[s] ?? null,
      ctx: () => ({
        extra: { vendorPrice: slot === null ? null : (v.cells[slot]?.price ?? null) },
      }),
    });
    hover(slot, r);
  }

  /** `OnChildNotify( 100, 100 )` of the grid: an item of the inventory dropped on a free slot of the own, closed shop. */
  #drop(sc: Shortcut, slot: number): boolean {
    const v = this.#deps.view.get();
    if (sc.kind !== SC.ITEM || sc.data === 0) return false;
    if (sc.from?.getFrameWnd()?.id !== APP_INVENTORY) return false;
    if (!v.owner || v.open) return false;
    const ok = this.#deps.actions.registerDrop(sc, slot);
    if (!ok) this.env.wm.drag.forbid();
    return ok;
  }

  // ---- notifications ---------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        const v = this.#deps.view.get();
        if (!v.owner || v.open) return true;
        if (this.#deps.actions.start(this.titleText.trim())) {
          this.#okHidden = true;
          this.okButton?.setVisible(false);
        }
        return true;
      }
      if (id === WIDC_CANCEL || id === WTBID_CLOSE) {
        // an open shop asks "stop selling?" first; otherwise the window just closes (`Destroy( FALSE )`)
        if (this.#deps.view.get().open) this.#deps.actions.cancel();
        else this.destroy();
        return true;
      }
      if (id === WIDC_RESET) {
        this.#deps.actions.reset();
        return true;
      }
      if (id === WIDC_CHAT) {
        this.#deps.actions.toggleChat();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#deps.tooltip.cancel();
    if (!this.#closedNotified) {
      this.#closedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}

export type { IconCellData };
