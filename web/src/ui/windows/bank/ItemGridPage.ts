// `ItemGridPage`: the tab page that holds ONE item grid (`m_wndItemCtrl[i].Create(WLVS_ICON, CRect(0, 0, 250, 250), pTabCtrl, 11)` of
// `CWndBank::OnInitialUpdate` and `CWndBagEx::OnInitialUpdate`; the inventory builds the same page for its bag). The page is a child of
// a tab control; the grid takes the size the tab gives the page. Shared by the bank and the pocket windows.

import { rect } from '../../layout/geometry';
import { ItemGrid, type ItemGridDelegate } from '../../controls/ItemGrid';
import { CodePage } from '../char/CodePage';

/** The window id of the grid inside the tab (`11`: what `WIN_ITEMDROP` carries as `nID` and `FindWindow` looks for). */
export const WIDC_ITEM_GRID = 11;

export class ItemGridPage extends CodePage {
  grid: ItemGrid | null = null;
  readonly #delegate: ItemGridDelegate;
  readonly #newFontColor: boolean;

  constructor(delegate: ItemGridDelegate, newFontColor = true) {
    super();
    this.#delegate = delegate;
    this.#newFontColor = newFontColor;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.grid = this.addControl(
      new ItemGrid(this.#delegate, this.#newFontColor),
      WIDC_ITEM_GRID,
      this.getClientRect(false),
    );
  }

  override onSize(type: number, cx: number, cy: number): void {
    super.onSize(type, cx, cy);
    this.grid?.setWndRect(rect(0, 0, cx, cy));
  }
}
