// `WndShop` (`CWndShop`, `src:_Interface/WndShop.cpp:700-950`, ref/22 §17 E, Plan/42 §19.4 E, Plan/45 §12; WP-UI-24):
// `APP_SHOPEX` (272 x 416). A tab control (`WIDC_INVENTORY`) with one page per vendor tab that has a title, each page the grid
// of its stock (`CWndItemCtrlVendor`, window ids 10..13, no stack counts); the two sort check boxes (by level, by gender)
// filter the displayed stock; `WIDC_COST` shows the price of the selected item; the Quick Sell button (`features.quick_sell`)
// opens `APP_QUICK_SELL`. Items are bought by dragging a stock item to the inventory grid (the inventory window routes the
// drop to the feature) or by double-clicking it; items are sold by dropping an inventory item on the shop. What a purchase
// or a sale means (gates, the quantity boxes, prices, the sends) is behind the ports.

import { ITYPE_ITEM } from '../../../gen/defs/define';
import {
  APP_INVENTORY,
  APP_SHOPEX,
  WIDC_BUTTON_QUICK_SELL,
  WIDC_CHECK2,
  WIDC_CHECK_LEVEL,
  WIDC_COST,
  WIDC_INVENTORY,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { ItemGrid, type ItemGridDelegate } from '../../controls/ItemGrid';
import type { Static } from '../../controls/Static';
import type { TabCtrl } from '../../controls/TabCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { type Point, type Rect, rect, rectWidth } from '../../layout/geometry';
import { WSS_MONEY, Wnm } from '../../layout/styles';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CodePage } from '../char/CodePage';
import type { ShopActions, ShopCell, ShopTabView, ShopView, ViewSource } from './ports';

/** `m_wndItemCtrl[i].Create(WLVS_ICON, .., pTabCtrl, i + 10)`: the window id of the grid of tab `i`. */
export const WIDC_TAB_GRID = 10;
/** The vendor has at most four tabs; the tab control shows at least three (`for (i = GetSize(); i < 3; ++i)`). */
export const MIN_TABS = 3;

export interface ShopDeps {
  readonly view: ViewSource<ShopView>;
  readonly actions: ShopActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

/** The tab page that holds the stock grid of one vendor tab. */
class StockPage extends CodePage {
  grid: ItemGrid | null = null;
  /** The container index (`BuyItem.tab`) of the tab. */
  tabIndex = 0;
  /** The stock entries the grid shows, in order (the filtered, compacted list). */
  shown: readonly ShopCell[] = [];
  readonly #delegate: ItemGridDelegate;
  readonly #id: number;

  constructor(delegate: ItemGridDelegate, id: number) {
    super();
    this.#delegate = delegate;
    this.#id = id;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.grid = this.addControl(
      new ItemGrid(this.#delegate, true),
      this.#id,
      this.getClientRect(false),
    );
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.grid?.setWndRect(rect(0, 0, cx, cy));
  }
}

export class WndShop extends WndNeuz {
  readonly appId = APP_SHOPEX;
  readonly #deps: ShopDeps;
  readonly #cells = new WeakMap<ShopCell, IconCellData>();
  readonly #pages: StockPage[] = [];
  #applied: ShopView | null = null;
  #tab: TabCtrl | null = null;
  #cost: Static | null = null;
  #quick: Button | null = null;
  #levelBox: Button | null = null;
  #sexBox: Button | null = null;
  #shownCost = '';
  #off: (() => void) | null = null;
  #destroyedNotified = false;

  constructor(deps: ShopDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  // ---- accessors for tests and the feature layer -------------------------------------------------------------------------

  get tab(): TabCtrl | null {
    return this.#tab;
  }
  get quickSellButton(): Button | null {
    return this.#quick;
  }
  get levelBox(): Button | null {
    return this.#levelBox;
  }
  get sexBox(): Button | null {
    return this.#sexBox;
  }
  get costText(): Static | null {
    return this.#cost;
  }
  /** The stock pages (one per vendor tab with a title), in tab order. */
  get pages(): readonly StockPage[] {
    return this.#pages;
  }
  /** The page of the visible tab, `null` for a blank tab. */
  get currentPage(): StockPage | null {
    const sel = this.#tab?.getCurSel() ?? 0;
    return this.#pages[sel] ?? null;
  }

  // ---- OnInitialUpdate ---------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#tab = this.getDlgItem<TabCtrl>(WIDC_INVENTORY);
    this.#cost = this.getDlgItem<Static>(WIDC_COST);
    this.#cost?.addWndStyle(WSS_MONEY);
    this.#quick = this.getDlgItem<Button>(WIDC_BUTTON_QUICK_SELL);
    this.#levelBox = this.getDlgItem<Button>(WIDC_CHECK_LEVEL);
    this.#sexBox = this.getDlgItem<Button>(WIDC_CHECK2);

    this.#buildTabs(this.#deps.view.get());
    this.#placeNextToInventory();
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  /** One page per vendor tab with a title, then blank tabs up to three (`CWndShop::OnInitialUpdate`). */
  #buildTabs(v: ShopView): void {
    const tab = this.#tab;
    if (!tab) return;
    let n = 0;
    for (const t of v.tabs) {
      if (t.title === '') continue;
      const page = new StockPage(this.#delegateOf(n), WIDC_TAB_GRID + t.index);
      page.attach(tab, WIDC_TAB_GRID + t.index);
      page.tabIndex = t.index;
      this.#pages.push(page);
      tab.insertItem(n, { text: t.title, page });
      n += 1;
    }
    for (let i = n; i < MIN_TABS; i += 1) tab.insertItem(i, { text: '', page: null });
  }

  /** The window opens beside the inventory: left of it when the inventory is on the right half, else right of it. */
  #placeNextToInventory(): void {
    const inv = this.env.wm.get(APP_INVENTORY);
    const mine = this.rectWindow;
    const w = rectWidth(mine);
    if (!inv) return;
    const r = inv.rectWindow;
    const pt: Point = r.l > w / 2 ? { x: r.l - w, y: r.t } : { x: r.l + rectWidth(r), y: r.t };
    this.move(pt);
  }

  #delegateOf(pageIndex: number): ItemGridDelegate {
    const page = (): StockPage | undefined => this.#pages[pageIndex];
    return {
      shortcutOf: (slot) => {
        const p = page();
        const cell = p?.shown[slot];
        if (!p || !cell) return null;
        // `userId` carries the container index of the tab: the inventory's drop handler needs to know which tab it came from
        return {
          kind: SC.ITEM,
          id: cell.objId,
          type: ITYPE_ITEM,
          index: slot,
          userId: p.tabIndex,
          data: 1,
          itemId: cell.itemId,
          ...(cell.icon === null ? {} : { icon: cell.icon }),
        };
      },
      press: (slot) => {
        this.#showCost(page()?.shown[slot] ?? null);
        return true;
      },
      dblClick: (slot) => {
        const p = page();
        const cell = p?.shown[slot];
        if (p && cell) this.#deps.actions.buy({ tab: p.tabIndex, cell });
      },
      hover: itemHover({
        tooltip: this.#deps.tooltip,
        owner: this,
        flag: APP_SHOPEX,
        itemAt: (slot) => page()?.shown[slot] ?? null,
      }),
      drop: (sc) => this.#dropItem(sc),
    };
  }

  // ---- the view ---------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  #cellOf(cell: ShopCell): IconCellData {
    let c = this.#cells.get(cell);
    if (!c) {
      // `m_bVisibleCount = FALSE`: a shop grid draws no stack counts
      c = { icon: cell.icon, count: cell.count, showCount: false, tint: cell.tint ? 'red' : null };
      this.#cells.set(cell, c);
    }
    return c;
  }

  /** The stock of a tab after the sort check boxes (`CWndItemCtrl::OnDraw` for `APP_SHOP_`). */
  #filtered(t: ShopTabView): ShopCell[] {
    const byLevel = this.#levelBox?.getCheck() === 1;
    const bySex = this.#sexBox?.getCheck() === 1;
    return t.cells.filter(
      (c) => (!bySex || c.sexMatch !== false) && (!byLevel || c.levelOk !== false),
    );
  }

  #apply(v: ShopView): void {
    this.#applied = v;
    this.#quick?.setVisible(v.quickSell);
    let n = 0;
    for (const t of v.tabs) {
      if (t.title === '') continue;
      const page = this.#pages[n];
      n += 1;
      if (!page) continue;
      const shown = this.#filtered(t);
      page.shown = shown;
      page.grid?.setNewFontColor(v.newFontColor);
      page.grid?.setCells(shown.map((c) => this.#cellOf(c)));
    }
    this.#refreshCost();
  }

  #refreshCost(): void {
    const page = this.currentPage;
    const slot = page?.grid?.selectedSlot ?? -1;
    this.#showCost(slot >= 0 ? (page?.shown[slot] ?? null) : null);
  }

  /** `WIDC_COST`: the price of the selected stock item, `0` without a selection. */
  #showCost(cell: ShopCell | null): void {
    const text = String(cell?.price ?? 0);
    if (text === this.#shownCost) return;
    this.#shownCost = text;
    this.#cost?.setTitle(text);
  }

  // ---- selling -------------------------------------------------------------------------------------------------------------------

  /** `WIN_ITEMDROP` of a tab grid (nID 10..13): an inventory item to sell. */
  #dropItem(sc: Shortcut): boolean {
    const from = sc.from?.getFrameWnd();
    if (sc.kind !== SC.ITEM || sc.type !== ITYPE_ITEM || sc.data === 0) return false;
    if (from?.id !== APP_INVENTORY) return false;
    const ok = this.#deps.actions.sell(sc.id);
    if (!ok) this.env.wm.drag.forbid();
    return ok;
  }

  /** A drop on the frame itself sells too (the whole window is the sell area). */
  override onDropIcon(sc: Shortcut): boolean {
    return this.#dropItem(sc);
  }

  // ---- notifications ---------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_CHECK_LEVEL || id === WIDC_CHECK2) {
        // a check box toggles before it notifies: the filter reads the new state (`m_bLevelSort = !m_bLevelSort`)
        this.#apply(this.#deps.view.get());
        return true;
      }
      if (id === WIDC_BUTTON_QUICK_SELL) {
        this.#deps.actions.quickSell();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#deps.tooltip.cancel();
    if (!this.#destroyedNotified) {
      this.#destroyedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}

export type { Rect };
