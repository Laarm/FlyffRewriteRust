// `TabCtrl` (`CWndTabCtrl`, WndControl.cpp:4219-4795; Plan/42 §7.6, ref/22 §6.5): tabs along the bottom 18 px of the
// control, one child window (`WndNeuz` created with `WBS_CHILD`) per tab, sized `rect - 18 px`. The strip is part of the
// baked frame skin (`skinOps`: panel + unselected strip + the controls of the visible page), the selected tab is drawn
// over it from `WndTabTile10-15.bmp`; captions are white (unselected) and black (selected), centred.
//
//   const page = new MyPage(); page.initialize(tab, WBS_CHILD | WBS_NODRAWFRAME);   // a layout window as the page
//   tab.insertItem(0, { text: 'General', page });
//
// A tab selects on a press in the strip (`x / (w / n)`), sends `SELCHANGE` to the parent and re-bakes the frame
// (the visible page's controls paint their skins). A tab without a page cannot be selected (the original reverts).

import type { Point, Rect, FrameRects } from '../layout/geometry';
import { rectHeight, rectOffset, rectWidth } from '../layout/geometry';
import type { ControlLayout } from '../layout/registry';
import { WBS_CHILDFRAME, WBS_NOFRAME, WBS_VSCROLL, WTYPE_TABCTRL, Wnm } from '../layout/styles';
import { controlSkinOp, type SkinOp } from '../skin/baker';
import { Control, type ControlViewState } from '../wnd/Control';
import type { WndBase } from '../wnd/WndBase';
import type { WndNeuz } from '../wnd/WndNeuz';
import type { Shortcut } from '../dnd/shortcut';
import { viewOf } from './bind';
import { TabCtrlView } from './TabCtrlView';
import { type TabAlign, TAB_STRIP_HEIGHT, tabAt, tabClickLen } from './tabMath';

/** `WTCITEM`: the caption and the page window of a tab. */
export interface TabItem {
  text: string;
  /** The page (a `WndNeuz` created as a child of this control); `null` = a tab without content. */
  page: WndNeuz | null;
}

export interface TabState extends ControlViewState {
  tabs: readonly TabItem[];
  /** `m_nCurSelect`. */
  cur: number;
  align: TabAlign;
}

export class TabCtrl extends Control<TabState> {
  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_TABCTRL, layout, { tabs: [], cur: 0, align: 'center' });
    this.view = viewOf<TabCtrl>(TabCtrlView);
  }

  /** `WBS_NOFRAME` control: the client rect is the window rect. */
  protected override computeRects(window: Rect): FrameRects {
    const copy = (): Rect => ({ ...window });
    return { window: copy(), client: copy(), layout: copy() };
  }

  override onInitialUpdate(): void {
    this.addWndStyle(WBS_NOFRAME); // `Create` adds `WBS_CHILD | WBS_NOFRAME`
  }

  // ---- API ----------------------------------------------------------------------------------------------------

  /**
   * `InsertItem(index, item)`: the page of the current tab is shown, the others hidden; every page takes the
   * rect `(0, 0, w, h - 18)` (`- 15` on the right with `WBS_VSCROLL`) and gets `WBS_CHILDFRAME`.
   */
  insertItem(index: number, item: TabItem): boolean {
    const tabs = this.state.get().tabs.slice();
    while (tabs.length < index) tabs.push({ text: '', page: null });
    tabs[index] = item;
    this.state.patch({ tabs });
    const { cur } = this.state.get();
    if (item.page) {
      item.page.setVisible(index === cur);
      item.page.setWndRect(this.#pageRect(true));
      item.page.addWndStyle(WBS_CHILDFRAME);
      // the original focuses the page of the current tab
      tabs[cur]?.page?.setFocus();
    }
    this.requestBake();
    return true;
  }

  /** The page rect: `(0, 0, w, h - 18)`, `- 15` on the right with `WBS_VSCROLL` (`withScroll`: InsertItem / SetCurSel). */
  #pageRect(withScroll: boolean): Rect {
    const w =
      rectWidth(this.rectWindowValue) - (withScroll && (this.style & WBS_VSCROLL) !== 0 ? 15 : 0);
    return { l: 0, t: 0, r: w, b: rectHeight(this.rectWindowValue) - TAB_STRIP_HEIGHT };
  }

  getCurSel(): number {
    return this.state.get().cur;
  }
  getItemCount(): number {
    return this.state.get().tabs.length;
  }
  getItem(index: number): TabItem | null {
    return this.state.get().tabs[index] ?? null;
  }
  /** `GetSelectedTab`. */
  getSelectedTab(): TabItem | null {
    return this.getItem(this.state.get().cur);
  }
  /** `SetTabTitleAlign`. */
  setTabTitleAlign(align: TabAlign): void {
    this.state.patch({ align });
  }

  /**
   * `SetCurSel(item)`: hides the old page, shows the new one, re-places every page, re-bakes the frame and focuses
   * the new page. A tab without a page keeps the old selection. Returns `item` (like the original).
   */
  setCurSel(item: number): number {
    const { tabs, cur: old } = this.state.get();
    const target = tabs[item];
    if (!target?.page) return item;
    tabs[old]?.page?.setVisible(false);
    this.state.patch({ cur: item });
    target.page.setVisible(true);
    const rect = this.#pageRect(true);
    for (const t of tabs) t.page?.setWndRect(rect);
    this.requestBake();
    target.page.setFocus();
    return item;
  }

  override onSize(): void {
    // `OnSize`: pages get `(0, 0, w, h - 18)` (no scroll-bar reduction here, KEEP)
    const rect = this.#pageRect(false);
    for (const t of this.state.get().tabs) t.page?.setWndRect(rect);
  }

  // ---- pointer ------------------------------------------------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    const tabs = this.state.get().tabs;
    const i = tabAt(
      pt.x,
      pt.y,
      rectWidth(this.rectWindowValue),
      rectHeight(this.rectWindowValue),
      tabs.length,
    );
    if (i < 0) return;
    this.setCurSel(i);
    this.parent?.onChildNotify(Wnm.SELCHANGE, this.id, this);
  }

  /** A press on the strip is the tab's own; elsewhere the pages take the press. */
  override isPickupSpace(ptWindow: Point): boolean {
    return super.isPickupSpace(ptWindow);
  }

  /** `OnChildNotify`: everything the pages report goes on to the parent. */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    return this.parent?.onChildNotify(msg, id, src) ?? false;
  }

  /** `m_nTabButtonLength` (tests). */
  clickLength(): number {
    return tabClickLen(rectWidth(this.rectWindowValue), this.state.get().tabs.length);
  }

  // ---- skin ---------------------------------------------------------------------------------------------------

  /**
   * `AdditionalSkinTexture`: the panel and unselected strip over the control's rect, then the skins of the controls
   * of the VISIBLE page at (tab origin + page client offset + control rect). (Plan §7.4 says tab-page controls get no
   * skin; the C++ bakes them and re-bakes on `SetCurSel`: source wins.)
   */
  override skinOps(): SkinOp[] {
    if (!this.visibleValue || this.destroyed) return [];
    const frame = this.getFrameWnd();
    const parent = this.parent;
    if (!frame || !parent) return [];
    const po = parent.isRoot
      ? { x: 0, y: 0 }
      : {
          x: parent.rectClient.l - parent.rectWindow.l,
          y: parent.rectClient.t - parent.rectWindow.t,
        };
    const origin = rectOffset(this.rectWindowValue, po.x, po.y);
    const { tabs, cur } = this.state.get();
    const ops: SkinOp[] = [{ op: 'tabs', rect: origin, tabs: Math.max(1, tabs.length) }];
    const page = tabs[cur]?.page;
    if (page && !page.destroyed && page.isVisible()) {
      const co = {
        x: page.rectClient.l - page.rectWindow.l,
        y: page.rectClient.t - page.rectWindow.t,
      };
      for (const child of page.controls) {
        if (child.destroyed || !child.isVisible()) continue;
        const r = rectOffset(
          child.rectWindow,
          origin.l + page.rectWindow.l + co.x,
          origin.t + page.rectWindow.t + co.y,
        );
        const op = controlSkinOp(child.kind, child.style, child.skinTexture, child.skinTile, r);
        if (op) ops.push(op);
      }
    }
    return ops;
  }
}
