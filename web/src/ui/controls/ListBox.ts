// `ListBox` (`CWndListBox`, WndControl.cpp:1805-2665; Plan/42 §8.8, ref/22 §8): single-selection list of rich-text
// items with a docking scroll bar (`WBS_VSCROLL`). Items start at (3, 3) of the client rect, the pitch is the font cell
// + 1 (16 px), the page is `clientH / pitch` lines. Colours (the whole item is recoloured, markup colours do not
// survive): normal `rgb(64,64,64)`, selected `rgb(64,64,255)`, hover `rgb(255,128,0)`, invalid `rgb(170,170,170)`.
//  * a press selects the item under the pointer (invalid items are skipped); the release over the selected item sends
//    `SELCHANGE` to the parent; a double click `DBLCLK` to the owning frame; a right release `SELCANCEL`;
//  * the wheel scrolls one line per notch (not while the page covers the range).
// FIX (not in the original): hidden items are skipped consistently when painting and hit-testing, and the scroll
// position is clamped when items are removed; an item is one line (the original wrapped long items into the next one).

import type { Point, Rect } from '../layout/geometry';
import { rectHeight, rectWidth } from '../layout/geometry';
import type { ControlLayout } from '../layout/registry';
import { WBS_CHILD, WBS_VSCROLL, WTYPE_LISTBOX, Wnm } from '../layout/styles';
import { FONT, lineHeight } from '../text/fonts';
import { parseMarkup, type RichRun, runsText } from '../text/markup';
import { Control, type ControlViewState } from '../wnd/Control';
import type { WndBase } from '../wnd/WndBase';
import { viewOf } from './bind';
import { ListBoxView } from './ListBoxView';
import { createOwnerScrollBar, ownerScrollBarRect, type ScrollBar } from './ScrollBar';
import { wheelActive, wheelPos } from './scrollMath';
import { SCROLLBAR_WIDTH } from './scrollMath';
import type { SkinOp } from '../skin/baker';

/** One list item (`LISTITEM`). */
export interface ListItem {
  /** The text as given (markup included). */
  text: string;
  /** The parsed markup (colours are replaced by the state colours when drawn). */
  runs: RichRun[];
  data: number;
  data2: number;
  key: string;
  /** `m_bIsValid`: an invalid item is grey and cannot be selected. */
  valid: boolean;
  /** `m_bIsVisible`. */
  visible: boolean;
}

export const LIST_COLORS = {
  font: 0xff404040,
  select: 0xff4040ff,
  hover: 0xffff8000,
  invalid: 0xffaaaaaa,
} as const;

export interface ListBoxState extends ControlViewState {
  items: readonly ListItem[];
  /** `m_nCurSelect` (-1 = none). */
  cur: number;
  /** The item under the pointer (-1 = none). */
  hover: number;
  /** The first visible row (the scroll position). */
  scroll: number;
  fontColor: number;
  selectColor: number;
  hoverColor: number;
  invalidColor: number;
  leftMargin: number;
}

/** `CWndListBox::FindString` (binary search over a SORTED list; -1 when the first letters differ). */
export function findStringIndex(items: readonly { text: string }[], text: string): number {
  if (items.length === 0) return -1;
  const cmp = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);
  let begin = 0;
  let end = items.length - 1;
  let div = begin + Math.trunc((end - begin) / 2);
  let c = 0;
  do {
    if (c === 1) end = div;
    if (c === -1) begin = div;
    div = begin + Math.trunc((end - begin) / 2);
    c = cmp(items[div]?.text ?? '', text);
    if (c === 0) return div;
  } while (end - begin > 1);
  let result: number;
  let item = items[begin];
  c = cmp(item?.text ?? '', text);
  if (c >= 0) result = begin;
  else {
    item = items[end];
    result = end;
  }
  if (text.at(0) !== item?.text.at(0)) result = -1;
  return result;
}

export class ListBox extends Control<ListBoxState> {
  /** The popup list of a combo box bakes its skin itself: the control then contributes none. */
  noSkin = false;
  #bar: ScrollBar | null = null;

  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_LISTBOX, layout, {
      items: [],
      cur: -1,
      hover: -1,
      scroll: 0,
      fontColor: LIST_COLORS.font,
      selectColor: LIST_COLORS.select,
      hoverColor: LIST_COLORS.hover,
      invalidColor: LIST_COLORS.invalid,
      leftMargin: 0,
    });
    this.view = viewOf<ListBox>(ListBoxView);
  }

  get scrollBar(): ScrollBar | null {
    return this.#bar;
  }

  /** `m_nFontHeight`: the row pitch (cell + `m_nLineSpace` 1). */
  pitch(): number {
    return lineHeight(FONT.text) + 1;
  }

  // ---- life cycle ---------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    const bar = createOwnerScrollBar(this);
    bar.onScroll = (pos) => {
      this.state.patch({ scroll: pos });
    };
    bar.forwardWheel = (n) => {
      this.onMouseWheel(n);
    };
    this.#bar = bar;
    bar.setVisible((this.style & WBS_VSCROLL) !== 0);
    this.#updateBar();
  }

  override onSize(): void {
    const bar = this.#bar;
    if (bar) {
      bar.setWndRect(ownerScrollBarRect(this));
      bar.setVisible((this.style & WBS_VSCROLL) !== 0);
    }
    this.#updateBar();
  }

  override skinOps(): SkinOp[] {
    return this.noSkin ? [] : super.skinOps();
  }

  // ---- geometry -----------------------------------------------------------------------------------------------

  /** The number of fully visible rows (`GetClientRect().Height() / m_nFontHeight`). */
  page(): number {
    return Math.max(0, Math.floor(rectHeight(this.rectClientValue) / this.pitch()));
  }

  /** The number of items that take a row. */
  visibleCount(): number {
    return this.state.get().items.filter((i) => i.visible).length;
  }

  /** The hit / paint rect of item `index` in CLIENT coordinates (`null` for a hidden or unknown item). */
  itemRect(index: number): Rect | null {
    const s = this.state.get();
    const item = s.items[index];
    if (!item?.visible) return null;
    let row = 0;
    for (let i = 0; i < index; i += 1) if (s.items[i]?.visible === true) row += 1;
    const barW = (this.style & WBS_VSCROLL) !== 0 ? SCROLLBAR_WIDTH : 0;
    const top = 3 + (row - s.scroll) * this.pitch();
    return { l: 3, t: top, r: 3 + rectWidth(this.rectWindowValue) - barW, b: top + this.pitch() };
  }

  /** The item under a client point (`-1` for none); invalid items count only when `includeInvalid`. */
  itemAt(pt: Point, includeInvalid = false): number {
    const items = this.state.get().items;
    for (let i = 0; i < items.length; i += 1) {
      const item = items[i];
      if (!item || (!item.valid && !includeInvalid)) continue;
      const r = this.itemRect(i);
      if (r && pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) return i;
    }
    return -1;
  }

  #updateBar(): void {
    const bar = this.#bar;
    if (!bar) return;
    if ((this.style & WBS_VSCROLL) !== 0) {
      bar.setScrollRange(0, this.visibleCount());
      bar.setScrollPage(this.page());
      const max = Math.max(0, bar.getMaxScrollPos() - bar.getScrollPage());
      if (bar.getScrollPos() > max) bar.setScrollPos(max);
    }
    this.state.patch({ scroll: bar.getScrollPos() });
  }

  // ---- API (CWndListBox) --------------------------------------------------------------------------------------

  /** `AddString`: appends an item (markup parsed once); returns its index. */
  addString(text: string): number {
    const item: ListItem = {
      text,
      runs: parseMarkup(text, LIST_COLORS.font),
      data: 0,
      data2: 0,
      key: '',
      valid: true,
      visible: true,
    };
    const items = [...this.state.get().items, item];
    this.state.patch({ items });
    this.#updateBar();
    return items.length - 1;
  }

  /** `InsertString`. */
  insertString(index: number, text: string): number {
    const at = Math.max(0, Math.min(index, this.state.get().items.length));
    const item: ListItem = {
      text,
      runs: parseMarkup(text, LIST_COLORS.font),
      data: 0,
      data2: 0,
      key: '',
      valid: true,
      visible: true,
    };
    const items = [...this.state.get().items];
    items.splice(at, 0, item);
    const cur = this.state.get().cur;
    this.state.patch({ items, cur: cur >= at ? cur + 1 : cur });
    this.#updateBar();
    return at;
  }

  /** `DeleteString`: removes an item; the selection is dropped when it was that item (the original does not shift it). */
  deleteString(index: number): boolean {
    const s = this.state.get();
    if (index < 0 || index >= s.items.length) return false;
    const items = s.items.filter((_it, i) => i !== index);
    this.state.patch({ items, cur: index === s.cur ? -1 : s.cur > index ? s.cur - 1 : s.cur });
    this.#updateBar();
    return true;
  }

  /** `ResetContent`. */
  resetContent(): void {
    this.state.patch({ items: [], cur: -1, hover: -1 });
    this.#bar?.setScrollPos(0);
    this.#updateBar();
  }

  getCount(): number {
    return this.state.get().items.length;
  }
  getCurSel(): number {
    return this.state.get().cur;
  }
  /** `SetCurSel`: returns the index, 0 for an index out of range (like the original). */
  setCurSel(index: number): number {
    if (index >= 0 && index < this.state.get().items.length) {
      this.state.patch({ cur: index });
      return index;
    }
    return 0;
  }
  /** `GetText`: the item text as given (markup included); `''` out of range. */
  getText(index: number): string {
    return this.state.get().items[index]?.text ?? '';
  }
  /** The text of an item without its markup. */
  getPlainText(index: number): string {
    const item = this.state.get().items[index];
    return item ? runsText(item.runs) : '';
  }
  getTextLen(index: number): number {
    return this.getText(index).length;
  }
  /** `SetString(index, text)`. */
  setString(index: number, text: string): void {
    this.#update(index, { text, runs: parseMarkup(text, LIST_COLORS.font) });
  }

  #update(index: number, patch: Partial<ListItem>): boolean {
    const items = this.state.get().items;
    const item = items[index];
    if (!item) return false;
    const next = items.slice();
    next[index] = { ...item, ...patch };
    this.state.patch({ items: next });
    if ('visible' in patch) this.#updateBar();
    return true;
  }

  setItemData(index: number, data: number): boolean {
    return this.#update(index, { data });
  }
  getItemData(index: number): number {
    return this.state.get().items[index]?.data ?? 0;
  }
  setItemData2(index: number, data2: number): boolean {
    return this.#update(index, { data2 });
  }
  getItemData2(index: number): number {
    return this.state.get().items[index]?.data2 ?? 0;
  }
  setKeyString(index: number, key: string): boolean {
    return this.#update(index, { key });
  }
  getKeyString(index: number): string {
    return this.state.get().items[index]?.key ?? '';
  }
  /** `SetItemValidity`: an invalid item is grey and not selectable. */
  setItemValidity(index: number, valid: boolean): boolean {
    return this.#update(index, { valid });
  }
  getItemValidity(index: number): boolean {
    return this.state.get().items[index]?.valid ?? false;
  }
  setItemVisibility(index: number, visible: boolean): boolean {
    return this.#update(index, { visible });
  }
  getItemVisibility(index: number): boolean {
    return this.state.get().items[index]?.visible ?? false;
  }
  /** `GetItemIndex(text | data)`: the first item with that exact text or data, -1 when none. */
  getItemIndex(match: string | number): number {
    const items = this.state.get().items;
    return items.findIndex((i) =>
      typeof match === 'string' ? i.text === match : i.data === match,
    );
  }
  /** `FindString(startAfter, text)` over a sorted list (see {@link findStringIndex}). */
  findString(_startAfter: number, text: string): number {
    return findStringIndex(this.state.get().items, text);
  }
  /**
   * `SelectString`: selects the item found by {@link findStringIndex} and scrolls it into view
   * (`pos = cur` or `range - page`).
   */
  selectString(startAfter: number, text: string): number {
    const cur = this.findString(startAfter, text);
    this.state.patch({ cur });
    if (cur === -1) return -1;
    const bar = this.#bar;
    if (bar) {
      const pos = bar.getScrollPos();
      const range = bar.getScrollRange();
      const page = bar.getScrollPage();
      if (cur < pos || cur > pos + page)
        bar.setScrollPos(cur < range - page ? Math.max(0, cur) : range - page);
    }
    return cur;
  }
  /** `SortListBox`: case-insensitive by text (`_stricmp`). */
  sortListBox(): void {
    const items = this.state.get().items.slice();
    items.sort((a, b) => {
      const x = a.text.toLowerCase();
      const y = b.text.toLowerCase();
      return x < y ? -1 : x > y ? 1 : 0;
    });
    this.state.patch({ items, cur: -1 });
  }
  setOnMouseColor(color: number): void {
    this.state.patch({ hoverColor: color >>> 0 });
  }
  setInvalidColor(color: number): void {
    this.state.patch({ invalidColor: color >>> 0 });
  }
  setSelectColor(color: number): void {
    this.state.patch({ selectColor: color >>> 0 });
  }
  setFontColor(color: number): void {
    this.state.patch({ fontColor: color >>> 0 });
  }
  setLeftMargin(margin: number): void {
    this.state.patch({ leftMargin: margin });
  }

  // ---- pointer ------------------------------------------------------------------------------------------------

  override onMouseMove(pt: Point): void {
    this.state.patch({ hover: this.itemAt(pt) });
  }
  override onMouseLeave(): void {
    this.state.patch({ hover: -1 });
  }

  override onLButtonDown(pt: Point): void {
    const i = this.itemAt(pt);
    if (i >= 0) this.state.patch({ cur: i });
  }

  override onLButtonUp(pt: Point): void {
    const i = this.itemAt(pt);
    if (i >= 0 && i === this.state.get().cur) {
      this.parent?.onChildNotify(Wnm.SELCHANGE, this.id, this);
    }
  }

  override onRButtonUp(pt: Point): void {
    const i = this.itemAt(pt);
    if (i >= 0 && i === this.state.get().cur) {
      this.parent?.onChildNotify(Wnm.SELCANCEL, this.id, this);
    }
  }

  override onLButtonDblClk(pt: Point): void {
    const i = this.itemAt(pt, true);
    if (i >= 0 && i === this.state.get().cur)
      this.#frameAncestor().onChildNotify(Wnm.DBLCLK, this.id, this);
  }

  /** `while (pWnd->GetStyle() & WBS_CHILD) pWnd = pWnd->GetParentWnd()`: the first non-child ancestor. */
  #frameAncestor(): WndBase {
    let p: WndBase | null = this.parent;
    while (p?.isWndStyle(WBS_CHILD) === true && p.parent) p = p.parent;
    return p ?? this;
  }

  override onMouseWheel(notches: number): void {
    const bar = this.#bar;
    if (!bar || !wheelActive(bar.getMaxScrollPos(), bar.getScrollPage())) return;
    bar.setScrollPos(
      wheelPos(
        notches,
        bar.getScrollPos(),
        bar.getMinScrollPos(),
        bar.getMaxScrollPos(),
        bar.getScrollPage(),
      ),
    );
  }
}
