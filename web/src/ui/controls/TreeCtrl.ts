// `TreeCtrl` (`CWndTreeCtrl`, WndControl.cpp:769-1455; Plan/42 §8.8, ref/22 §8): a tree of rich-text elements with
// open / close icons (`buttTreeOpen.tga` / `buttTreeClose.tga`, 8 x 9) at the start of each line that has children, the
// keyword at `x + 16` (`m_nCategoryTextSpace`), 16 px of indentation per level (`m_nTreeTabWidth`), a row pitch of one
// cell (15 px; selected = the element's select colour, default blue `rgb(0,0,255)`, normal `rgb(64,64,64)`).
//  * a press on an icon toggles the element and selects it; a release over a keyword selects it and sends
//    `CLICKED` to the owning frame; a double click toggles and sends `DBLCLK`;
//  * the wheel scrolls one line per notch while the page is smaller than the range.
// FIX: the original scrolls by `cell + 3` px per line while painting rows `cell` px apart; both use the row pitch here.
// Not built: the member-check-box mode (`m_bMemberCheckingMode`, party / guild invitations).

import type { Point, Rect } from '../layout/geometry';
import { rectHeight, rectWidth } from '../layout/geometry';
import type { ControlLayout } from '../layout/registry';
import { WBS_CHILD, WBS_VSCROLL, WTYPE_TREECTRL, Wnm } from '../layout/styles';
import { FONT, lineHeight, textWidth } from '../text/fonts';
import { parseMarkup, type RichRun, runsText } from '../text/markup';
import { Control, type ControlViewState } from '../wnd/Control';
import type { WndBase } from '../wnd/WndBase';
import { viewOf } from './bind';
import { createOwnerScrollBar, ownerScrollBarRect, type ScrollBar } from './ScrollBar';
import { wheelPos } from './scrollMath';
import { peekSprite } from './sprites';
import { TreeCtrlView } from './TreeCtrlView';

/** `TREEELEM`. */
export interface TreeNode {
  /** A unique id (React keys). */
  id: number;
  /** The keyword as given (markup included) and parsed. */
  keyword: string;
  runs: RichRun[];
  data: number;
  open: boolean;
  /** `m_dwColor` / `m_dwSelectColor` (`0xAARRGGBB`). */
  color: number;
  selectColor: number;
  parent: TreeNode | null;
  children: TreeNode[];
}

/** The default colours of an element (`tagScriptElem`). */
export const TREE_COLOR = 0xff404040;
export const TREE_SELECT_COLOR = 0xff0000ff;

/** A line of the flattened tree with its place in client coordinates. */
export interface TreeRow {
  node: TreeNode;
  depth: number;
  /** The icon cell (left of the keyword) and the keyword's rect. */
  icon: Rect;
  text: Rect;
}

export interface TreeState extends ControlViewState {
  /** Bumps whenever a node changed (nodes are mutable objects). */
  version: number;
  focus: TreeNode | null;
  scroll: number;
}

let nextNodeId = 0;
const ICON_W = 8;
const ICON_H = 9;

export class TreeCtrl extends Control<TreeState> {
  readonly roots: TreeNode[] = [];
  #bar: ScrollBar | null = null;
  tabWidth = 16;
  categoryTextSpace = 16;

  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_TREECTRL, layout, { version: 0, focus: null, scroll: 0 });
    this.view = viewOf<TreeCtrl>(TreeCtrlView);
  }

  get scrollBar(): ScrollBar | null {
    return this.#bar;
  }

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
    this.#refresh();
  }

  override onSize(): void {
    const bar = this.#bar;
    if (bar) {
      bar.setWndRect(ownerScrollBarRect(this));
      bar.setVisible((this.style & WBS_VSCROLL) !== 0);
    }
    this.#refresh();
  }

  /** The row pitch (`GetFontHeight()`). */
  pitch(): number {
    return lineHeight(FONT.text);
  }

  // ---- model --------------------------------------------------------------------------------------------------

  /** `InsertItem(parent, text, data, ...)`: appends under `parent` (`null` = a root element). */
  insertItem(
    parent: TreeNode | null,
    text: string,
    data = 0,
    color: number = TREE_COLOR,
    selectColor: number = TREE_SELECT_COLOR,
  ): TreeNode {
    nextNodeId += 1;
    const node: TreeNode = {
      id: nextNodeId,
      keyword: text,
      runs: parseMarkup(text, color),
      data,
      open: false,
      color: color >>> 0,
      selectColor: selectColor >>> 0,
      parent,
      children: [],
    };
    (parent ? parent.children : this.roots).push(node);
    this.#refresh();
    return node;
  }

  /** `DeleteAllItems`. */
  deleteAllItems(): void {
    this.roots.length = 0;
    this.state.patch({ focus: null });
    this.#bar?.setScrollPos(0);
    this.#refresh();
  }

  getCurSel(): TreeNode | null {
    return this.state.get().focus;
  }
  getFocusElem(): TreeNode | null {
    return this.state.get().focus;
  }
  setFocusElem(node: TreeNode | null): void {
    this.state.patch({ focus: node, version: this.state.get().version + 1 });
  }
  getRootElem(): readonly TreeNode[] {
    return this.roots;
  }
  /** `FindTreeElem(data | keyword)`: depth first. */
  findTreeElem(match: number | string): TreeNode | null {
    const walk = (nodes: readonly TreeNode[]): TreeNode | null => {
      for (const n of nodes) {
        if (typeof match === 'number' ? n.data === match : n.keyword === match) return n;
        const inner = walk(n.children);
        if (inner) return inner;
      }
      return null;
    };
    return walk(this.roots);
  }
  /** `SetCurSel(data | keyword)`: selects the found element (or none). */
  setCurSel(match: number | string): TreeNode | null {
    const n = this.findTreeElem(match);
    this.state.patch({ focus: n, version: this.state.get().version + 1 });
    return n;
  }
  /** Opens or closes an element (and refreshes the scroll range). */
  setOpen(node: TreeNode, open: boolean): void {
    node.open = open;
    this.#refresh();
  }
  /** `GetTreeItemsNumber`: all elements, open or not. */
  getTreeItemsNumber(): number {
    const count = (nodes: readonly TreeNode[]): number =>
      nodes.reduce((n, e) => n + 1 + count(e.children), 0);
    return count(this.roots);
  }
  /** `SetTextColor(category, normal, selectedCategory, selectedNormal)`: roots and parents are categories. */
  setTextColor(
    category: number,
    normal: number,
    selectedCategory: number,
    selectedNormal: number,
  ): void {
    const walk = (nodes: readonly TreeNode[]): void => {
      for (const n of nodes) {
        if (n.children.length > 0 || n.parent === null) {
          n.color = category >>> 0;
          n.selectColor = selectedCategory >>> 0;
          walk(n.children);
        } else {
          n.color = normal >>> 0;
          n.selectColor = selectedNormal >>> 0;
        }
      }
    };
    walk(this.roots);
    this.#refresh();
  }
  setTreeTabWidth(w: number): void {
    this.tabWidth = w;
    this.#refresh();
  }
  setCategoryTextSpace(w: number): void {
    this.categoryTextSpace = w;
    this.#refresh();
  }

  // ---- rows ---------------------------------------------------------------------------------------------------

  /** The visible lines (open elements expanded) with their rects in client coordinates at the current scroll. */
  rows(): TreeRow[] {
    const out: TreeRow[] = [];
    const cell = this.pitch();
    const scroll = this.state.get().scroll;
    const icon = peekSprite(this.env, 'buttTreeOpen.tga');
    const iw = typeof icon === 'object' ? icon.width : ICON_W;
    const ih = typeof icon === 'object' ? icon.height : ICON_H;
    let index = 0;
    const walk = (nodes: readonly TreeNode[], depth: number): void => {
      for (const n of nodes) {
        const x = 3 + depth * this.tabWidth;
        const y = 3 + (index - scroll) * cell;
        index += 1;
        const left = x + this.categoryTextSpace;
        out.push({
          node: n,
          depth,
          icon: { l: x, t: y, r: x + iw, b: y + ih },
          text: { l: left, t: y, r: left + textWidth(FONT.text, runsText(n.runs)), b: y + cell },
        });
        if (n.children.length > 0 && n.open) walk(n.children, depth + 1);
      }
    };
    walk(this.roots, 0);
    return out;
  }

  /** How many lines the tree has (the scroll range). */
  rowCount(): number {
    let n = 0;
    const walk = (nodes: readonly TreeNode[]): void => {
      for (const e of nodes) {
        n += 1;
        if (e.children.length > 0 && e.open) walk(e.children);
      }
    };
    walk(this.roots);
    return n;
  }

  page(): number {
    return Math.max(0, Math.floor(rectHeight(this.rectClientValue) / this.pitch()));
  }

  /** Recomputes the scroll bar (`OnDraw`: range = lines, page; a position under an inactive bar goes to 0) and re-renders. */
  #refresh(): void {
    const bar = this.#bar;
    if (bar && (this.style & WBS_VSCROLL) !== 0) {
      const count = this.rowCount();
      bar.setScrollRange(0, count);
      bar.setScrollPage(this.page());
      if (bar.getScrollPage() >= bar.getMaxScrollPos()) bar.setScrollPos(0);
      else {
        const max = bar.getMaxScrollPos() - bar.getScrollPage();
        if (bar.getScrollPos() > max) bar.setScrollPos(max);
      }
    }
    this.state.patch({
      version: this.state.get().version + 1,
      scroll: bar ? bar.getScrollPos() : 0,
    });
  }

  // ---- pointer ------------------------------------------------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    for (const row of this.rows()) {
      const r = row.icon;
      if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
        row.node.open = !row.node.open;
        this.state.patch({ focus: row.node });
        this.#refresh();
        return;
      }
    }
  }

  override onLButtonUp(pt: Point): void {
    for (const row of this.rows()) {
      const r = row.text;
      if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
        this.state.patch({ focus: row.node, version: this.state.get().version + 1 });
        this.#frameAncestor().onChildNotify(Wnm.CLICKED, this.id, this);
        return;
      }
    }
  }

  override onLButtonDblClk(pt: Point): void {
    for (const row of this.rows()) {
      const r = row.text;
      if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
        row.node.open = !row.node.open;
        this.state.patch({ focus: row.node });
        this.#refresh();
        this.#frameAncestor().onChildNotify(Wnm.DBLCLK, this.id, this);
        return;
      }
    }
  }

  /** The first non-child ancestor (`while (pWnd->GetStyle() & WBS_CHILD)`). */
  #frameAncestor(): WndBase {
    let p: WndBase | null = this.parent;
    while (p?.isWndStyle(WBS_CHILD) === true && p.parent) p = p.parent;
    return p ?? this;
  }

  override onMouseWheel(notches: number): void {
    const bar = this.#bar;
    if (!bar) return;
    if (bar.getScrollPage() >= bar.getMaxScrollPos()) return; // active only while the page is smaller than the range
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

  /** The width of the tree view (for the view). */
  clientWidth(): number {
    return rectWidth(this.rectClientValue);
  }
}
