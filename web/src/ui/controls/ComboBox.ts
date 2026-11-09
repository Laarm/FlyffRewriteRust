// `ComboBox` (`CWndComboBox : CWndEdit`, WndControl.cpp:4796-5100; Plan/42 §8.8, ref/22 §8): the text of the selected
// item, a drop button `ButtQuickListDn.tga` (docking, 20 px at the right) and a popup list (`CWndComboListBox`, a
// `WBS_POPUP` FRAME in the top layer that hides when it loses the focus). A press anywhere on the combo toggles the
// list; picking an item puts its text into the combo, closes the list and sends `SELCHANGE` to the parent.
// The popup is `height = items * (cell + 3) + 8` high, as wide as the combo, below it. The text is read-only (the
// original lets a combo edit its text; no layout needs it).

import { type Point, type Rect, rectContains, rectHeight, rectWidth } from '../layout/geometry';
import type { ControlLayout } from '../layout/registry';
import {
  WBS_CHILD,
  WBS_DOCKING,
  WBS_NOFOCUS,
  WBS_NOFRAME,
  WBS_NODRAWFRAME,
  WBS_POPUP,
  WBS_VSCROLL,
  WTYPE_COMBOBOX,
  Wnm,
} from '../layout/styles';
import type { BakeSpec } from '../skin/baker';
import { fillRect, type PixelBuffer } from '../skin/pixels';
import { FONT, lineHeight } from '../text/fonts';
import { Control, type ControlViewState } from '../wnd/Control';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import { Button } from './Button';
import { viewOf } from './bind';
import { ComboBoxView } from './ComboBoxView';
import { ListBox } from './ListBox';

/** `m_wndButton` of the combo: the drop button (id 0). */
export const COMBO_BUTTON_ID = 0;
/** The popup list (`m_wndListBox`, id 1). */
export const COMBO_LIST_ID = 1;
/** The width of the drop button (`rect.left = rect.right - 20`). */
export const COMBO_BUTTON_WIDTH = 20;

/** `RenderWndEditFrame`: grey 1 px border (200,200,200) with cut corners and a white field. */
export function paintEditFrame(buf: PixelBuffer, w: number, h: number): void {
  const grey: [number, number, number, number] = [200, 200, 200, 255];
  fillRect(buf, 1, 0, w - 1, 1, grey);
  fillRect(buf, 1, h - 1, w - 1, h, grey);
  fillRect(buf, 0, 1, 1, h - 1, grey);
  fillRect(buf, w - 1, 1, w, h - 1, grey);
  fillRect(buf, 1, 1, w - 1, h - 1, [255, 255, 255, 255]);
}

/** The popup of a combo box: a frame with the baked `WndEditTile00` skin and one list box filling it. */
export class ComboPopup extends WndNeuz {
  readonly appId = -1;
  list: ListBox | null = null;
  readonly #combo: ComboBox;

  constructor(combo: ComboBox) {
    super();
    this.#combo = combo;
  }

  override onInitialUpdate(): void {
    // no layout: the frame holds the list only (`CWndComboListBox` is the list itself)
    const list = new ListBox();
    list.noSkin = true;
    this.list = this.addControl(list, COMBO_LIST_ID, this.#listRect(), WBS_VSCROLL | WBS_NOFRAME);
  }

  #listRect(): Rect {
    const r = this.rectWindowValue;
    return { l: 0, t: 0, r: rectWidth(r), b: rectHeight(r) };
  }

  override onSize(): void {
    this.list?.setWndRect(this.#listRect(), true);
    this.requestBake();
  }

  /**
   * `CWndComboListBox::PaintFrame` = `CTheme::RenderWndEditFrame`: a white field with a 1 px grey border whose
   * corners are cut (the baked tile skin of the list is replaced by it).
   */
  override buildBakeSpec(): BakeSpec | null {
    const r = this.rectWindowValue;
    const w = rectWidth(r);
    const h = rectHeight(r);
    return {
      width: w,
      height: h,
      ops: [],
      after: (ctx) => {
        paintEditFrame(ctx.buffer, w, h);
      },
    };
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number): boolean {
    if (id === COMBO_LIST_ID && msg === Wnm.SELCHANGE) {
      this.#combo.onListPicked();
      return true;
    }
    return false;
  }

  override onKillFocus(newWnd: WndBase | null): void {
    super.onKillFocus(newWnd);
    this.#combo.onListKillFocus();
  }
}

export interface ComboState extends ControlViewState {
  /** The text shown in the combo (the picked item's text, markup removed). */
  display: string;
  /** `m_bOpen`. */
  open: boolean;
}

export class ComboBox extends Control<ComboState> {
  #popup: ComboPopup | null = null;
  #button: Button | null = null;
  /** Items added before the popup exists (the popup is created with the first need). */
  #pending: { text: string; data: number }[] = [];

  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_COMBOBOX, layout, { display: '', open: false });
    this.view = viewOf<ComboBox>(ComboBoxView);
  }

  get dropButton(): Button | null {
    return this.#button;
  }
  /** The popup frame (created on first use). */
  get popup(): ComboPopup | null {
    return this.#popup;
  }

  override onInitialUpdate(): void {
    const w = rectWidth(this.rectWindowValue);
    const h = rectHeight(this.rectWindowValue);
    const button = new Button();
    button.setTexture('ButtQuickListDn.tga');
    button.setTitle('V');
    button.create(
      WBS_CHILD | WBS_DOCKING,
      { l: w - COMBO_BUTTON_WIDTH, t: 0, r: w, b: h },
      this,
      COMBO_BUTTON_ID,
    );
    this.#button = button;
  }

  override onSize(): void {
    const w = rectWidth(this.rectWindowValue);
    const h = rectHeight(this.rectWindowValue);
    this.#button?.setWndRect({ l: w - COMBO_BUTTON_WIDTH, t: 0, r: w, b: h });
  }

  override onDestroy(): void {
    if (this.#popup && !this.#popup.destroyed) this.#popup.destroy();
    super.onDestroy();
  }

  /** The list of the popup; the popup is created on demand. */
  #list(): ListBox {
    const popup = this.#ensurePopup();
    if (!popup.list) throw new Error('the combo popup has no list');
    return popup.list;
  }

  #ensurePopup(): ComboPopup {
    if (this.#popup && !this.#popup.destroyed) return this.#popup;
    const popup = new ComboPopup(this);
    // WBS_NOFOCUS: the lazily created popup must not take the focus from the window (the original creates it
    // inside the window's own `OnInitialUpdate`, where the window regains the focus right after)
    popup.create(
      WBS_POPUP | WBS_NOFRAME | WBS_NODRAWFRAME | WBS_NOFOCUS,
      { l: 0, t: 0, r: 1, b: 1 },
      this,
      COMBO_LIST_ID,
    );
    popup.setVisible(false);
    this.#popup = popup;
    for (const p of this.#pending.splice(0)) {
      const i = popup.list?.addString(p.text) ?? -1;
      if (i >= 0) popup.list?.setItemData(i, p.data);
    }
    return popup;
  }

  // ---- API (CWndComboBox) -----------------------------------------------------------------------------------

  /** `AddString`: returns the index. */
  addString(text: string): number {
    return this.#list().addString(text);
  }
  deleteString(index: number): boolean {
    return this.#list().deleteString(index);
  }
  insertString(index: number, text: string): number {
    return this.#list().insertString(index, text);
  }
  /** `ResetContent`: clears the items and the text. */
  resetContent(): void {
    this.state.patch({ display: '' });
    this.#list().resetContent();
  }
  getCount(): number {
    return this.#popup?.list?.getCount() ?? 0;
  }
  getCurSel(): number {
    return this.#popup?.list?.getCurSel() ?? -1;
  }
  /** `SetCurSel`: shows the text of the item (no notification). */
  setCurSel(index: number): number {
    const list = this.#list();
    if (index < 0 || index >= list.getCount()) return 0;
    this.state.patch({ display: list.getPlainText(index) });
    return list.setCurSel(index);
  }
  setItemData(index: number, data: number): boolean {
    return this.#list().setItemData(index, data);
  }
  getItemData(index: number): number {
    return this.#popup?.list?.getItemData(index) ?? 0;
  }
  /** `GetSelectedItemData`: the data of the selected item, 0 when none. */
  getSelectedItemData(): number {
    const list = this.#popup?.list;
    if (!list) return 0;
    const cur = list.getCurSel();
    return cur === -1 ? 0 : list.getItemData(cur);
  }
  getListBoxText(index: number): string {
    return this.#popup?.list?.getPlainText(index) ?? '';
  }
  getListBoxTextLength(index: number): number {
    return this.#popup?.list?.getTextLen(index) ?? 0;
  }
  getListBoxSize(): number {
    return this.getCount();
  }
  /** `SelectItem(text | data)`: selects the first match and sends `SELCHANGE` (nothing when none matches). */
  selectItem(match: string | number): void {
    const list = this.#list();
    const index = list.getItemIndex(match);
    if (index === -1) return;
    list.setCurSel(index);
    this.#picked();
  }
  /** `SetString`: the text shown in the combo. */
  setString(text: string): void {
    this.state.patch({ display: text });
  }
  getString(): string {
    return this.state.get().display;
  }
  isOpen(): boolean {
    return this.state.get().open;
  }

  // ---- opening and closing ----------------------------------------------------------------------------------

  /** `OpenListBox`: the popup below the combo, shown and focused. */
  openList(): void {
    const popup = this.#ensurePopup();
    const list = popup.list;
    if (!list) return;
    const sr = this.getScreenRect();
    const w = rectWidth(this.rectWindowValue);
    const h = list.getCount() * (lineHeight(FONT.text) + 3) + 8;
    popup.setWndRect({ l: sr.l, t: sr.b, r: sr.l + w, b: sr.b + h }, true);
    popup.setVisible(true);
    popup.requestBake();
    popup.setFocus();
  }

  #close(): void {
    this.#popup?.setVisible(false);
    this.state.patch({ open: false });
  }

  /** A press anywhere on the combo toggles the list (`OnLButtonDown`, VER15). */
  override onLButtonDown(): void {
    this.#toggle();
  }

  #toggle(): void {
    if (this.state.get().open) {
      this.state.patch({ open: false });
    } else {
      this.state.patch({ open: true });
      this.openList();
    }
  }

  /** The drop button was clicked (`OnChildNotify(WNM_CLICKED, 0)`). */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number): boolean {
    if (id === COMBO_BUTTON_ID && msg === Wnm.CLICKED) this.#toggle();
    return true;
  }

  /** The popup list reported a pick (its `SELCHANGE`). */
  onListPicked(): void {
    if (!this.state.get().open) return;
    this.#picked();
    this.setFocus();
    this.#close();
  }

  /** `SetSelectedItemInformation`: show the text of the picked item and tell the parent. */
  #picked(): void {
    const list = this.#list();
    const cur = list.getCurSel();
    this.state.patch({ display: cur === -1 ? '' : list.getPlainText(cur) });
    this.parent?.onChildNotify(Wnm.SELCHANGE, this.id, this);
  }

  /**
   * The popup lost the focus (`WNM_KILLFOCUS` of the list): when the pointer is over the combo or its button the
   * combo stays "open" so that the press that caused it toggles the list off instead of reopening it.
   */
  onListKillFocus(): void {
    const mouse = this.env.wm.lastPointer;
    const inside = mouse !== null && rectContains(this.getScreenRect(), mouse);
    this.state.patch({ open: inside });
  }

  /** For the view: the client origin offset of the text. */
  clientOrigin(): Point {
    return {
      x: this.rectClientValue.l - this.rectWindowValue.l,
      y: this.rectClientValue.t - this.rectWindowValue.t,
    };
  }
}
