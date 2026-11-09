// `Menu` (`CWndMenu`, WndControl.cpp:3469-3768; Plan/42 §8.13, ref/22 §8): a popup FRAME (`WBS_POPUP`, top layer) of
// `WBS_MENUITEM` buttons with a 22 px pitch, filled `rgb(226,198,181)` with a white client rect (3 px inset). Items
// are 20 px high at `(2, 2 + n * 22)`, all as wide as the widest caption + 60. A sub-menu (`item.setMenu(menu)`) opens
// to the right of its item when the pointer enters it; the whole menu hides when the focus moves to a window that is
// not another menu. A click is reported to the OWNER (the window passed to `createMenu`): `onCommand(id, msg, menu)` and
// `onChildNotify`, after which the owner takes the focus back (which closes the menu). Every resize of the menu
// (append, insert, delete, delete-all) re-bakes its skin, like the original's `SetWndRect(..., bOnSize = TRUE)`.

import type { FrameRects, Rect } from '../layout/geometry';
import { rectDeflate, rectHeight, rectWidth } from '../layout/geometry';
import { WBS_HIGHLIGHT, WBS_MENUITEM, WBS_POPUP } from '../layout/styles';
import type { BakeSpec } from '../skin/baker';
import { fillRect } from '../skin/pixels';
import { FONT, textWidth } from '../text/fonts';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import { Button } from './Button';

/** The pitch of the items (`22`). */
export const MENU_PITCH = 22;
/** The id `CreateMenu` gives the window (`10`). */
export const MENU_ID = 10;

/** The outermost menu of a chain of sub-menus (`SetVisibleAllMenu` walks up while the parent is a popup). */
function rootMenu(menu: Menu): Menu {
  const p = menu.parent;
  return p instanceof Menu && p.isWndStyle(WBS_POPUP) ? rootMenu(p) : menu;
}

export class Menu extends WndNeuz {
  readonly appId = -2;
  readonly items: Button[] = [];
  #largeWidth = 0;

  /** `CreateMenu(parent)`: creates the (hidden) popup under `parent`, the owner that receives the commands. */
  createMenu(parent: WndBase): boolean {
    const ok = this.create(WBS_POPUP, { l: 0, t: 0, r: 0, b: 0 }, parent, MENU_ID);
    this.setVisible(false);
    return ok;
  }

  /** The client rect is the window deflated by 3 (`CWndMenu::SetWndRect`). */
  protected override computeRects(window: Rect): FrameRects {
    const client = rectDeflate(window, 3, 3, 3, 3);
    return { window: { ...window }, client, layout: { ...client } };
  }

  override onInitialUpdate(): void {
    // no layout, no title bar
  }

  override onSize(): void {
    this.requestBake();
  }

  /** `PaintFrame` + `OnEraseBkgnd`: the window in `rgb(226,198,181)`, the client rect white. */
  override buildBakeSpec(): BakeSpec | null {
    const r = this.rectWindowValue;
    const w = rectWidth(r);
    const h = rectHeight(r);
    // an empty menu (before its first item, after `deleteAllMenu`) publishes no skin instead of a 0x0 one
    if (w <= 0 || h <= 0) return null;
    const c = this.rectClientValue;
    const co = { x: c.l - r.l, y: c.t - r.t };
    return {
      width: w,
      height: h,
      ops: [],
      after: (ctx) => {
        fillRect(ctx.buffer, 0, 0, w, h, [226, 198, 181, 255]);
        fillRect(
          ctx.buffer,
          co.x,
          co.y,
          co.x + rectWidth(c),
          co.y + rectHeight(c),
          [255, 255, 255, 255],
        );
      },
    };
  }

  // ---- items --------------------------------------------------------------------------------------------------

  /** `AppendMenu(text, id)`: a new item at the end; the menu grows. Returns the item. */
  appendMenu(text: string, id: number): Button {
    const n = this.items.length;
    const w = textWidth(FONT.text, text) + 60;
    if (w > this.#largeWidth) this.#largeWidth = w;
    const item = new Button();
    this.addControl(
      item,
      id,
      { l: 2, t: 2 + n * MENU_PITCH, r: this.#largeWidth, b: 2 + n * MENU_PITCH + 20 },
      WBS_MENUITEM | WBS_HIGHLIGHT,
    );
    item.setTitle(text);
    item.onHoverEnter = () => {
      this.#hover(item);
    };
    this.items.push(item);
    this.#layoutItems();
    this.setWndRect(
      { l: 0, t: 0, r: this.#largeWidth, b: 5 + 5 + this.items.length * MENU_PITCH },
      true,
    );
    return item;
  }

  /** `InsertMenu(pos, text, id)`: inserts at `pos` and shifts the following items down. */
  insertMenu(pos: number, text: string, id: number): Button {
    const w = textWidth(FONT.text, text) + 60;
    if (w > this.#largeWidth) this.#largeWidth = w;
    const item = new Button();
    this.addControl(
      item,
      id,
      { l: 2, t: 2, r: this.#largeWidth, b: 22 },
      WBS_MENUITEM | WBS_HIGHLIGHT,
    );
    item.setTitle(text);
    item.onHoverEnter = () => {
      this.#hover(item);
    };
    this.items.splice(Math.max(0, Math.min(pos, this.items.length)), 0, item);
    this.#layoutItems();
    this.setWndRect(
      {
        ...this.rectWindowValue,
        b: this.rectWindowValue.t + 5 + 5 + this.items.length * MENU_PITCH,
      },
      true,
    );
    return item;
  }

  #layoutItems(): void {
    this.items.forEach((it, i) => {
      const t = 2 + i * MENU_PITCH;
      it.setWndRect({ l: 2, t, r: 2 + this.#largeWidth, b: t + 20 });
    });
  }

  /** `DeleteMenu(pos)`. */
  deleteMenu(pos: number): boolean {
    const item = this.items[pos];
    if (!item) return false;
    this.items.splice(pos, 1);
    this.removeControl(item);
    this.#layoutItems();
    this.setWndRect(
      {
        ...this.rectWindowValue,
        b: this.rectWindowValue.t + 5 + 5 + this.items.length * MENU_PITCH,
      },
      true,
    );
    return true;
  }

  /** `DeleteAllMenu`. */
  deleteAllMenu(): void {
    for (const it of this.items.splice(0)) {
      this.removeControl(it);
    }
    this.#largeWidth = 0;
    this.setWndRect({ l: 0, t: 0, r: 0, b: 0 }, true);
  }

  getMenuItem(pos: number): Button | null {
    return this.items[pos] ?? null;
  }
  getMenuItemCount(): number {
    return this.items.length;
  }
  /** `CheckMenuItem(pos, check)`. */
  checkMenuItem(pos: number, check: boolean): void {
    this.items[pos]?.setCheck(check);
  }
  /** `GetMenuState(pos)`: 1 when the item is checked. */
  getMenuState(pos: number): number {
    return this.items[pos]?.getCheck() ?? 0;
  }

  // ---- visibility and sub-menus ---------------------------------------------------------------------------------

  /** `SetVisibleSub`: shows or hides this menu and every sub-menu below it. */
  setVisibleSub(visible: boolean): void {
    this.setVisible(visible);
    for (const it of this.items) {
      if (it.menu instanceof Menu) it.menu.setVisibleSub(visible);
    }
  }

  /** `SetVisibleAllMenu`: the root menu of the chain of popup parents and everything below it. */
  setVisibleAllMenu(visible: boolean): void {
    const menu = rootMenu(this);
    menu.setVisible(visible);
    for (const it of menu.items) {
      if (it.menu instanceof Menu) it.menu.setVisibleSub(visible);
    }
    for (const it of menu.items) it.setMenuOpen(it.menu?.isVisible() ?? false);
  }

  /** Focus loss closes the whole menu unless the focus went to another popup (a sub-menu). */
  override onKillFocus(newWnd: WndBase | null): void {
    // not `super.onKillFocus`: the base class hides the popup at once, a menu keeps its chain open
    if (this.env.wm.captureOwner === this) this.releaseCapture();
    if (!newWnd?.isWndStyle(WBS_POPUP)) this.setVisibleAllMenu(false);
  }

  /** The pointer entered `item`: hide the other sub-menus and open this item's (`CWndTaskMenu::Process`). */
  #hover(item: Button): void {
    const sub = item.menu;
    if (!sub?.isVisible()) {
      for (const it of this.items) {
        if (it.menu instanceof Menu) it.menu.setVisibleSub(false);
        it.setMenuOpen(false);
      }
    }
    if (sub instanceof Menu) {
      const rcButton = item.getScreenRect();
      sub.move({ x: rcButton.r, y: rcButton.t });
      const rcMenu = sub.getScreenRect();
      const layout = this.env.wm.layoutRect;
      const pt = { x: rcMenu.l, y: rcMenu.t };
      if (rcMenu.r > layout.r) pt.x = rcButton.l - rectWidth(rcMenu);
      if (rcMenu.b > layout.b) pt.y -= rcMenu.b - layout.b;
      sub.move(pt);
      sub.setVisible(true);
      item.setMenuOpen(true);
      sub.setFocus();
    }
  }

  // ---- commands go to the owner -------------------------------------------------------------------------------

  override onCommand(id: number, msg: number, src?: WndBase): boolean;
  override onCommand(id: number, msg: number): boolean {
    const owner = this.parent;
    if (!owner) return false;
    const r = owner.onCommand(id, msg, this);
    owner.setFocus();
    return r;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const owner = this.parent;
    if (!owner) return false;
    const r = owner.onChildNotify(msg, id, src);
    owner.setFocus();
    return r;
  }
}
