// `SlotGrid`: the base of the specialised icon grids (WP-UI-23; Plan/42 §8.10): the vendor grid (`CWndVendorCtrl`), the trade
// grid (`CWndTradeCtrl`) and the guild merit list (`CWndGuildMeritCtrl`). They differ from the inventory `ItemGrid` in that
// the slots sit at hand-written positions (the vendor has two columns of 180 px rows, the trade five columns, the merit list
// six columns with an offset), in what a press does and in what is drawn beside the icon. This class owns the common
// behaviour of the originals:
//   * a press on an occupied slot selects it and ARMS a drag (the drag starts on the first pointer move, not on a click) when
//     the delegate hands out a payload; nothing happens while another shortcut is being dragged;
//   * a double click, a right click and the hover of an occupied slot are reported to the delegate (the tooltip, the buy or
//     unregister action);
//   * a shortcut dropped on the grid is offered to the delegate with the slot under the pointer. The originals only accept a
//     drop on an EMPTY slot (`GetItem(i) == NULL`): `dropOnEmptyOnly` keeps that rule here, the grid answers `true` (the drop
//     is consumed) either way, as the C++ does;
//   * a grid with a scroll bar scrolls one row per wheel notch.
// What an item is, what it costs and what the gates are is the window's: the grid draws cells and reports.

import {
  type FrameRects,
  type Point,
  type Rect,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../layout/geometry';
import { WTYPE_CUSTOM, Wnm } from '../layout/styles';
import type { IconCellData } from '../icons/IconCell';
import type { Shortcut } from '../dnd/shortcut';
import { Control, type ControlViewState } from '../wnd/Control';
import { createOwnerScrollBar, type ScrollBar } from '../controls/ScrollBar';
import { viewOf } from '../controls/bind';
import { type SlotLayout, slotAtPoint } from './geometry';
import { SlotGridView } from './SlotGridView';

/** What the window provides (the C++ reads the parent frame and the player for these decisions). */
export interface SlotGridDelegate {
  /**
   * The payload of a drag from `slot` (built at the first pointer move after the press); `null` = not draggable (the item
   * is in use, nobody may take it out). The grid sets `from`.
   */
  shortcutOf?(slot: number): Shortcut | null;
  /** A press on an occupied slot; return `false` to refuse the selection and the drag (`GetAsyncKeyState(VK_LCONTROL)`). */
  press?(slot: number): boolean;
  /** `OnLButtonDblClk` on an occupied slot. */
  dblClick?(slot: number): void;
  /** `OnRButtonUp` on an occupied slot (the vendor's own grid unregisters the item). */
  rightClick?(slot: number): void;
  /** The pointer entered an occupied slot or left the cells: put or cancel the item tooltip (`rect` = the hit rect on screen). */
  hover?(slot: number | null, rect: Rect | null): void;
  /** A shortcut was dropped on `slot` (`dropOnEmptyOnly`: an empty one). Returns whether it was taken. */
  drop?(sc: Shortcut, slot: number): boolean;
}

export interface SlotGridOptions {
  readonly layout: SlotLayout;
  /** The control paints the translucent frame of `PaintFrame` (black 100/255 fill, a light rounded rectangle). */
  readonly frame: boolean;
  /** A docked scroll bar and the wheel scroll the rows (the merit list); fixed grids have neither. */
  readonly scrolls: boolean;
  /** The colour of the selection rectangle (`0xff00ffff` cyan of the vendor and merit list, white of the trade grid). */
  readonly selectColor: string;
  /** The selection rectangle relative to the icon origin: `[dx, dy, w, h]`. */
  readonly selectRect: readonly [number, number, number, number];
  /** Only an empty slot takes a drop. */
  readonly dropOnEmptyOnly: boolean;
  /** `features.new_font_color` for the stack counts. */
  readonly newFontColor: boolean;
}

export interface SlotGridState extends ControlViewState {
  /** The cells by slot (`null` = empty). */
  cells: readonly (IconCellData | null)[];
  /** The hovered occupied slot, `-1` = none. */
  hover: number;
  /** The selected slot (`m_nCurSel`), `-1` = none. */
  selected: number;
  /** The first visible row. */
  scroll: number;
  /** The visible rows (a scrolling grid: client height / row pitch). */
  page: number;
  clientOffset: Point;
}

export class SlotGrid<C extends IconCellData = IconCellData> extends Control<SlotGridState> {
  readonly options: SlotGridOptions;
  #delegate: SlotGridDelegate;
  #bar: ScrollBar | null = null;
  #pending = -1;

  constructor(options: SlotGridOptions, delegate: SlotGridDelegate = {}) {
    super(WTYPE_CUSTOM, null, {
      cells: [],
      hover: -1,
      selected: -1,
      scroll: 0,
      page: Math.max(1, Math.ceil(options.layout.count / options.layout.columns)),
      clientOffset: { x: 0, y: 0 },
    });
    this.options = options;
    this.#delegate = delegate;
    this.view = viewOf<SlotGrid>(SlotGridView);
  }

  setDelegate(delegate: SlotGridDelegate): void {
    this.#delegate = delegate;
  }

  /** The window is the client (`SetWndRect`: `m_rectClient = m_rectWindow`); a scrolling grid leaves 15 px for its bar. */
  protected override computeRects(window: Rect): FrameRects {
    const client: Rect = { ...window };
    if (this.options.scrolls) client.r -= 15;
    return { window: { ...window }, client, layout: { ...client } };
  }

  override onInitialUpdate(): void {
    if (this.options.scrolls) {
      const bar = createOwnerScrollBar(this);
      bar.onScroll = (pos) => {
        this.#setScroll(pos);
      };
      bar.forwardWheel = (n) => {
        this.onMouseWheel(n);
      };
      this.#bar = bar;
    }
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

  /** Pushes the cells (one per slot, `null` = empty); a selection on a slot that became empty is cleared. */
  setCells(cells: readonly (C | null)[]): void {
    const s = this.state.get();
    const selected = s.selected >= 0 && cells[s.selected] ? s.selected : -1;
    const hover = s.hover >= 0 && cells[s.hover] ? s.hover : -1;
    this.state.patch({ cells, selected, hover });
    this.#relayout();
  }

  get cells(): readonly (C | null)[] {
    return this.state.get().cells as readonly (C | null)[];
  }
  get selectedSlot(): number {
    return this.state.get().selected;
  }
  get hoverSlot(): number {
    return this.state.get().hover;
  }
  get scrollRow(): number {
    return this.state.get().scroll;
  }
  get visibleRows(): number {
    return this.state.get().page;
  }
  get totalRows(): number {
    return Math.ceil(this.options.layout.count / this.options.layout.columns);
  }

  select(slot: number): void {
    this.state.patch({ selected: slot });
  }

  /** `m_nCurSel = -1`. */
  clearSelection(): void {
    this.state.patch({ selected: -1 });
  }

  /** Scrolls so that `row` is the first visible row (clamped; no effect on a fixed grid). */
  scrollTo(row: number): void {
    this.#bar?.setScrollPos(this.#clamp(row));
  }

  #clamp(row: number): number {
    const max = Math.max(0, this.totalRows - this.state.get().page);
    return Math.max(0, Math.min(max, row));
  }

  #setScroll(pos: number): void {
    this.state.patch({ scroll: this.#clamp(pos) });
  }

  #relayout(): void {
    if (!this.options.scrolls) return;
    const pitch = this.options.layout.rowPitch;
    const page = Math.max(1, Math.floor(rectHeight(this.rectClientValue) / pitch));
    this.state.patch({ page });
    const bar = this.#bar;
    if (bar) {
      bar.setScrollRange(0, this.totalRows);
      bar.setScrollPage(page);
      bar.setVisible(this.totalRows > page);
    }
    this.#setScroll(bar?.getScrollPos() ?? this.state.get().scroll);
  }

  // ---- geometry ----------------------------------------------------------------------------------------------

  /** The slot under a point of the client rect (any slot, empty ones included), or `-1`. */
  slotAt(pt: Point): number {
    return slotAtPoint(this.options.layout, pt, this.state.get().scroll);
  }

  /** The hit rect of a slot in root (virtual screen) coordinates, `null` when the slot is not in a visible row. */
  slotScreenRect(slot: number): Rect | null {
    const s = this.state.get();
    const { layout } = this.options;
    const row = Math.floor(slot / layout.columns);
    if (slot < 0 || slot >= layout.count || row < s.scroll || row > s.scroll + s.page) return null;
    const sc = this.getScreenClientRect();
    const r = layout.hit(slot);
    return rectOffset(r, sc.l, sc.t - s.scroll * layout.rowPitch);
  }

  /** The icon origin of a slot in client coordinates at the current scroll (what the view draws at). */
  slotOrigin(slot: number): Point {
    const o = this.options.layout.origin(slot);
    return { x: o.x, y: o.y - this.state.get().scroll * this.options.layout.rowPitch };
  }

  // ---- pointer -----------------------------------------------------------------------------------------------

  #cell(slot: number): C | null {
    return slot >= 0 ? ((this.state.get().cells[slot] ?? null) as C | null) : null;
  }

  override onLButtonDown(pt: Point): void {
    // `CWndBase::m_GlobalShortcut.IsEmpty() == FALSE` returns: nothing happens while a shortcut is dragged
    if (this.env.wm.isDraggingShortcut()) return;
    const slot = this.slotAt(pt);
    if (slot < 0 || !this.#cell(slot)) return;
    if (this.#delegate.press?.(slot) === false) return;
    this.state.patch({ selected: slot });
    if (!this.#delegate.shortcutOf) return;
    this.#pending = slot;
    this.env.wm.drag.arm(this, () => {
      const p = this.#pending;
      this.#pending = -1;
      if (p < 0) return null;
      const sc = this.#delegate.shortcutOf?.(p) ?? null;
      return sc ? { ...sc, from: this } : null;
    });
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
    const next = slot >= 0 && this.#cell(slot) ? slot : -1;
    if (next === this.state.get().hover) return;
    this.state.patch({ hover: next });
    this.#delegate.hover?.(next >= 0 ? next : null, next >= 0 ? this.slotScreenRect(next) : null);
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
    if (slot < 0 || !this.#cell(slot)) return;
    this.state.patch({ selected: slot });
    this.#delegate.dblClick?.(slot);
    this.parent?.onChildNotify(Wnm.WIN_DBLCLK, this.id, this);
  }

  override onRButtonUp(pt: Point): void {
    const slot = this.slotAt(pt);
    if (slot < 0 || !this.#cell(slot)) return;
    this.state.patch({ selected: slot });
    this.#delegate.rightClick?.(slot);
  }

  override onDropIcon(sc: Shortcut, pt: Point): boolean {
    // `pt` is relative to the control's window; the C++ hit-tests the client point
    const c = this.rectClientValue;
    const w = this.rectWindowValue;
    const slot = this.slotAt({ x: pt.x - (c.l - w.l), y: pt.y - (c.t - w.t) });
    if (slot < 0) return true;
    if (this.options.dropOnEmptyOnly && this.#cell(slot)) return true;
    this.#delegate.drop?.(sc, slot);
    return true;
  }

  override onMouseWheel(notches: number): void {
    if (!this.options.scrolls) return;
    this.scrollTo(this.state.get().scroll - Math.sign(notches));
  }
}
