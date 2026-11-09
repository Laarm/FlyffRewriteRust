// `WndQuickSell` (`CWndQuickSell` + `CWndQuickSellCtrl`, `src:_Interface/WndShop.cpp:960-1250`, ref/22 §17 E, Plan/45 §12.4;
// WP-UI-24): `APP_QUICK_SELL` (262 x 504). The items that a quick sale takes (a grid over the rect of `WIDC_LISTBOX1`, six
// per row, a docked scroll bar), the total in `WIDC_STATIC3` (thousands separators) and Confirm / Cancel. The items are
// reserved by the feature while the window is open; destroying the window in ANY way restores them (the destructor of the
// control calls `RestoreItem`). The total is `Σ rules.quickSellUnitPrice` (PL-168: a bound item previews at 1 penya).

import {
  APP_QUICK_SELL,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_LISTBOX1,
  WIDC_STATIC3,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { ItemGrid, type ItemGridDelegate } from '../../controls/ItemGrid';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import type { IconCellData } from '../../icons/IconCell';
import { Wnm } from '../../layout/styles';
import { formatMoney } from '../../text/format';
import { itemHover } from '../../tooltip/itemHover';
import type { TooltipService } from '../../tooltip/TooltipService';
import { APP_INVENTORY } from '../../../gen/defs/resdata';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QuickSellActions, QuickSellEntry, QuickSellView, ViewSource } from './ports';

/** The window id of the grid (`m_wndctrlMerit.Create(.., 100)`). */
export const WIDC_QUICK_GRID = 100;

export interface QuickSellDeps {
  readonly view: ViewSource<QuickSellView>;
  readonly actions: QuickSellActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class WndQuickSell extends WndNeuz {
  readonly appId = APP_QUICK_SELL;
  readonly #deps: QuickSellDeps;
  readonly #cells = new WeakMap<QuickSellEntry, IconCellData>();
  #grid: ItemGrid | null = null;
  #total: Static | null = null;
  #view: QuickSellView | null = null;
  #off: (() => void) | null = null;
  #closedNotified = false;

  constructor(deps: QuickSellDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get grid(): ItemGrid | null {
    return this.#grid;
  }
  get totalText(): Static | null {
    return this.#total;
  }
  get confirmButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#total = this.getDlgItem<Static>(WIDC_STATIC3);
    const area = this.getWndCtrl(WIDC_LISTBOX1)?.rect;
    if (area) {
      const delegate: ItemGridDelegate = {
        // nothing is dragged out of the quick-sell list
        shortcutOf: () => null,
        hover: itemHover({
          tooltip: this.#deps.tooltip,
          owner: this,
          flag: APP_INVENTORY,
          itemAt: (slot) => this.#view?.entries[slot] ?? null,
        }),
      };
      this.#grid = this.addControl(new ItemGrid(delegate, true), WIDC_QUICK_GRID, area);
    }
    this.moveParentCenter();
    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  #cellOf(e: QuickSellEntry): IconCellData {
    let c = this.#cells.get(e);
    if (!c) {
      c = { icon: e.icon, count: e.count, showCount: e.showCount };
      this.#cells.set(e, c);
    }
    return c;
  }

  #apply(v: QuickSellView): void {
    this.#view = v;
    this.#grid?.setCells(v.entries.map((e) => this.#cellOf(e)));
    this.#total?.setTitle(formatMoney(v.total));
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#view) this.#apply(v);
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.#deps.actions.confirm();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON2) {
      this.destroy();
      return true;
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
