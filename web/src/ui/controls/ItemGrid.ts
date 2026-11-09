// `ItemGrid` (`CWndItemCtrl`, WndItemCtrl.cpp; Plan/42 §8.10; WP-UI-12): a grid of 32 x 32 icon cells on a 32 px pitch
// (tooltip hit rect 30 x 30), laid out row by row in the slots of a container, with a docked scroll bar and the wheel
// scrolling one row per notch. Created by code inside a tab page (the inventory) or a frame (bag, bank, shop):
//
//   const grid = frame.addControl(new ItemGrid(delegate), 11, rect);
//   grid.setCells(cells);                  // the window pushes its view here (<= 10 Hz)
//
// Behaviour of the original, ported:
//   * the client rect is the window deflated by 4 px left and right (`SetWndRect`) minus 15 px for the scroll bar, so
//     `columns = floor(clientWidth / 32)` and a row is a block of `columns` consecutive slots; `page = floor(h / 32)` rows;
//   * a press on an item selects it and ARMS a drag; the drag starts on the FIRST pointer move after the press (not on a
//     click) with the payload `delegate.shortcutOf(slot)` (a `Shortcut` of kind ITEM whose `from` is this grid);
//   * a double click on an item sends `WIN_DBLCLK` with the grid's id to the parent (and calls `delegate.dblClick`);
//   * a shortcut dropped on the grid calls `delegate.drop(shortcut, slot)` (`-1`: the pointer is over no slot, the C++
//     appends: `nDstIndex = m_dwIndexNum`);
//   * the hovered cell is drawn 4 px larger (`m_nOnSelect`) and the delegate may put the item tooltip (30 x 30 rect).
// What the items mean (equip rules, gates, the tooltip composer) is the window's: the grid only draws and reports.

import {
  type FrameRects,
  type Point,
  type Rect,
  rectDeflate,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../layout/geometry';
import { WTYPE_CUSTOM, Wnm } from '../layout/styles';
import type { IconCellData } from '../icons/IconCell';
import type { Shortcut } from '../dnd/shortcut';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { ItemGridView } from './ItemGridView';
import { createOwnerScrollBar, type ScrollBar } from './ScrollBar';

/** The cell pitch and the tooltip hit rect (`rectHittest = (x * 32, y * 32, x * 32 + 30, y * 32 + 30)`). */
export const CELL = 32;
export const CELL_HIT = 30;

/** What the window shell provides (the C++ read the parent frame's id for these decisions). */
export interface ItemGridDelegate {
  /**
   * The payload of a drag from `slot`, built at the first pointer move after the press; `null` = not draggable (the
   * item is in use, a modal dialog forbids it, ...). The grid sets `from`.
   */
  shortcutOf(slot: number): Shortcut | null;
  /** A press on an occupied slot (`OnLButtonDown`); return `false` to refuse the selection and the drag. */
  press?(slot: number): boolean;
  /** `OnRButtonDown` on the grid (the upgrade cursor of the inventory is cancelled by it). */
  rightPress?(slot: number): void;
  /** `OnLButtonDblClk` on an occupied slot (before `WIN_DBLCLK` is sent to the parent). */
  dblClick?(slot: number): void;
  /** The pointer entered an occupied slot (`slot`) or left the cells (`null`): put or cancel the item tooltip. */
  hover?(slot: number | null, screenRect: Rect | null): void;
  /** A shortcut was dropped: on `slot`, or on no slot (`-1`). Returns whether the drop was accepted. */
  drop?(sc: Shortcut, slot: number): boolean;
}

export interface ItemGridState extends ControlViewState {
  /** The cells by slot index (`null` = empty slot). */
  cells: readonly (IconCellData | null)[];
  /** Total number of slots (the container size, empty slots included). */
  size: number;
  /** The first visible row (the scroll position). */
  scroll: number;
  columns: number;
  /** Visible rows (`page`). */
  rows: number;
  /** The hovered slot (`m_nOnSelect`), `-1` = none. */
  hover: number;
  /** The selected slot (`m_nCurSelect`), `-1` = none. */
  selected: number;
  newFontColor: boolean;
  /** The client rect relative to the window (where the cells' origin is). */
  clientOffset: Point;
}

/** `columns` for a client width. */
export function gridColumns(clientWidth: number): number {
  return Math.max(1, Math.floor(clientWidth / CELL));
}

/** Rows needed for `size` slots (`nRange`). */
export function gridRange(size: number, columns: number): number {
  return Math.ceil(size / Math.max(1, columns));
}

/** The slot at a point of the client rect (`HitTest`: 32 x 32 cells), or `-1`. Empty slots count. */
export function gridSlotAt(
  pt: Point,
  columns: number,
  scroll: number,
  size: number,
  rows: number,
): number {
  if (pt.x < 0 || pt.y < 0) return -1;
  const col = Math.floor(pt.x / CELL);
  const row = Math.floor(pt.y / CELL);
  if (col >= columns || row >= Math.max(rows, 0) + 1) return -1;
  const slot = (scroll + row) * columns + col;
  return slot >= 0 && slot < size ? slot : -1;
}

export class ItemGrid extends Control<ItemGridState> {
  #delegate: ItemGridDelegate;
  #bar: ScrollBar | null = null;
  #pending = -1;

  constructor(delegate: ItemGridDelegate, newFontColor = true) {
    super(WTYPE_CUSTOM, null, {
      cells: [],
      size: 0,
      scroll: 0,
      columns: 1,
      rows: 1,
      hover: -1,
      selected: -1,
      newFontColor,
      clientOffset: { x: 0, y: 0 },
    });
    this.#delegate = delegate;
    this.view = viewOf<ItemGrid>(ItemGridView);
  }

  setDelegate(d: ItemGridDelegate): void {
    this.#delegate = d;
  }

  /** `SetWndRect`: client = window deflated (4, 0), minus 15 px on the right for the scroll bar. */
  protected override computeRects(window: Rect): FrameRects {
    const c = rectDeflate(window, 4, 0, 4, 0);
    const client: Rect = { ...c, r: c.r - 15 };
    return { window: { ...window }, client, layout: client };
  }

  override onInitialUpdate(): void {
    const bar = createOwnerScrollBar(this);
    bar.onScroll = (pos) => {
      this.#setScroll(pos);
    };
    bar.forwardWheel = (n) => {
      this.onMouseWheel(n);
    };
    this.#bar = bar;
    this.#relayout();
  }

  override onSize(): void {
    this.#bar?.setWndRect({
      l: rectWidth(this.rectWindowValue) - 15,
      t: 0,
      r: rectWidth(this.rectWindowValue),
      b: rectHeight(this.rectWindowValue),
    });
    this.#relayout();
  }

  protected override onRectChanged(): void {
    super.onRectChanged();
    const o = {
      x: this.rectClientValue.l - this.rectWindowValue.l,
      y: this.rectClientValue.t - this.rectWindowValue.t,
    };
    const cur = this.state.get().clientOffset;
    if (cur.x !== o.x || cur.y !== o.y) this.state.patch({ clientOffset: o });
  }

  get scrollBar(): ScrollBar | null {
    return this.#bar;
  }

  // ---- the model ---------------------------------------------------------------------------------------------

  /** Pushes the cells (one per slot of the container; `null` = empty). */
  setCells(cells: readonly (IconCellData | null)[]): void {
    this.state.patch({ cells, size: cells.length });
    this.#relayout();
  }

  setNewFontColor(on: boolean): void {
    this.state.patch({ newFontColor: on });
  }

  get columns(): number {
    return this.state.get().columns;
  }
  get visibleRows(): number {
    return this.state.get().rows;
  }
  get scrollRow(): number {
    return this.state.get().scroll;
  }
  /** `m_nCurSelect` (the slot the last press selected), `-1` = none. */
  get selectedSlot(): number {
    return this.state.get().selected;
  }
  /** Rows of the whole container (`nRange`). */
  get totalRows(): number {
    const s = this.state.get();
    return gridRange(s.size, s.columns);
  }

  /** Scrolls so that `row` is the first visible row (clamped). */
  scrollTo(row: number): void {
    this.#bar?.setScrollPos(this.#clamp(row));
  }

  #clamp(row: number): number {
    const s = this.state.get();
    const max = Math.max(0, gridRange(s.size, s.columns) - s.rows);
    return Math.max(0, Math.min(max, row));
  }

  #setScroll(pos: number): void {
    this.state.patch({ scroll: this.#clamp(pos) });
  }

  /** Recomputes columns, page and the scroll bar after a size or content change (`OnDraw` does it every frame). */
  #relayout(): void {
    const cw = rectWidth(this.rectClientValue);
    const columns = gridColumns(cw);
    const rows = Math.max(1, Math.floor(rectHeight(this.rectWindowValue) / CELL));
    const size = this.state.get().size;
    const range = gridRange(size, columns);
    this.state.patch({ columns, rows });
    const bar = this.#bar;
    if (bar) {
      bar.setScrollRange(0, range);
      bar.setScrollPage(rows);
      bar.setVisible(range > rows);
    }
    this.#setScroll(bar?.getScrollPos() ?? this.state.get().scroll);
  }

  // ---- geometry helpers ----------------------------------------------------------------------------------------

  /** The slot under a point of the client rect, or `-1`. */
  slotAt(pt: Point): number {
    const s = this.state.get();
    return gridSlotAt(pt, s.columns, s.scroll, s.size, s.rows);
  }

  /** The 30 x 30 tooltip rect of a slot in root (virtual screen) coordinates, or `null` when it is not visible. */
  cellScreenRect(slot: number): Rect | null {
    const s = this.state.get();
    const index = slot - s.scroll * s.columns;
    if (slot < 0 || index < 0) return null;
    const row = Math.floor(index / s.columns);
    if (row > s.rows) return null;
    const col = index % s.columns;
    const sc = this.getScreenClientRect();
    return rectOffset(
      { l: 0, t: 0, r: CELL_HIT, b: CELL_HIT },
      sc.l + col * CELL,
      sc.t + row * CELL,
    );
  }

  // ---- pointer ---------------------------------------------------------------------------------------------------

  override onLButtonDown(pt: Point): void {
    // `CWndBase::m_GlobalShortcut.IsEmpty() == FALSE` returns: nothing happens while a shortcut is dragged
    if (this.env.wm.isDraggingShortcut()) return;
    const slot = this.slotAt(pt);
    const cell = slot >= 0 ? (this.state.get().cells[slot] ?? null) : null;
    if (slot < 0 || !cell) {
      // an empty slot clears the selection (`m_dwSelectAry.RemoveAll()`)
      this.state.patch({ selected: slot });
      return;
    }
    if (this.#delegate.press?.(slot) === false) return;
    this.state.patch({ selected: slot });
    this.#pending = slot;
    // the drag starts at the first pointer move (`m_bDrag` + `OnMouseMove`)
    this.env.wm.drag.arm(this, () => {
      const p = this.#pending;
      this.#pending = -1;
      if (p < 0) return null;
      const sc = this.#delegate.shortcutOf(p);
      return sc ? { ...sc, from: this } : null;
    });
  }

  override onRButtonDown(pt: Point): void {
    this.#delegate.rightPress?.(this.slotAt(pt));
  }

  override onLButtonUp(): void {
    this.#pending = -1;
  }
  override onPointerRelease(): void {
    this.#pending = -1;
  }

  override onMouseMove(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const slot = this.slotAt(pt);
    const s = this.state.get();
    const cell = slot >= 0 ? (s.cells[slot] ?? null) : null;
    // the hit rect of the tooltip is 30 x 30 inside the 32 x 32 cell
    const inHit = cell !== null && slot >= 0 && pt.x % CELL < CELL_HIT && pt.y % CELL < CELL_HIT;
    const next = inHit ? slot : -1;
    if (next === s.hover) return;
    this.state.patch({ hover: next });
    this.#delegate.hover?.(next >= 0 ? next : null, next >= 0 ? this.cellScreenRect(next) : null);
  }

  override onMouseLeave(): void {
    if (this.state.get().hover >= 0) {
      this.state.patch({ hover: -1 });
      this.#delegate.hover?.(null, null);
    }
  }

  override onLButtonDblClk(pt: Point): void {
    if (this.env.wm.isDraggingShortcut()) return;
    const slot = this.slotAt(pt);
    if (slot < 0 || !this.state.get().cells[slot]) return;
    this.state.patch({ selected: slot });
    this.#delegate.dblClick?.(slot);
    this.parent?.onChildNotify(Wnm.WIN_DBLCLK, this.id, this);
  }

  override onDropIcon(sc: Shortcut, pt: Point): boolean {
    // `pt` is relative to the control's window; the C++ hit-tests the client point
    const c = this.rectClientValue;
    const w = this.rectWindowValue;
    const slot = this.slotAt({ x: pt.x - (c.l - w.l), y: pt.y - (c.t - w.t) });
    return this.#delegate.drop?.(sc, slot) ?? false;
  }

  override onMouseWheel(notches: number): void {
    // one row per notch: wheel up scrolls toward the first row
    this.scrollTo(this.state.get().scroll - Math.sign(notches));
  }
}
