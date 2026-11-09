// `ScrollBar` (`CWndScrollBar`, WndControl.cpp:1455-1800; Plan/42 §8.9, ref/22 §6.7): the vertical scroll bar of the
// Text, ListBox and Tree controls. It is a docking child of its owner (rect `(w - 15, 0, w, h)` of the owner's window
// rect); the model is in lines (`min`, `max`, `page`, `pos`); the arrows step one line (no push-repeat, KEEP), the
// track pages, the thumb drags, the wheel is the owner's. The two arrow buttons of the original are parts of this
// control (same pixels and states: hover 0, normal 1, pushed 2 of `ButtVScrUp.tga` / `ButtVScrDown.tga`).

import { type Point, type Rect, rectWidth, rectHeight } from '../layout/geometry';
import { WBS_CHILD, WBS_DOCKING, WTYPE_SCROLLBAR } from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import type { FrameRects } from '../layout/geometry';
import type { WndBase } from '../wnd/WndBase';
import { pagePos, posFromDrag, SCROLLBAR_WIDTH, stepPos, thumbGeometry } from './scrollMath';
import { ScrollBarView } from './ScrollBarView';

/** The id of the scroll bar of a control (`m_wndScrollBar.Create(WBS_DOCKING | WBS_VERT, rect, this, 1000)`). */
export const SCROLLBAR_ID = 1000;

export type ScrollPart = 'up' | 'down' | 'thumb' | 'track';

export interface ScrollBarState extends ControlViewState {
  min: number;
  max: number;
  page: number;
  pos: number;
  hover: ScrollPart | null;
  pressed: ScrollPart | null;
}

export class ScrollBar extends Control<ScrollBarState> {
  /** Called with the new position whenever it changes (the owner re-lays out and re-renders its lines). */
  onScroll: ((pos: number) => void) | null = null;
  /** Wheel notches over the bar are the owner's business (`CWndBase::OnMouseWheel` of the bar does nothing). */
  forwardWheel: ((notches: number) => void) | null = null;
  #grab = 0;
  #dragging = false;

  constructor() {
    super(WTYPE_SCROLLBAR, null, { min: 0, max: 0, page: 0, pos: 0, hover: null, pressed: null });
    this.view = viewOf<ScrollBar>(ScrollBarView);
  }

  /** Client = window (`CWndScrollBar::SetWndRect`). */
  protected override computeRects(window: Rect): FrameRects {
    const copy = (): Rect => ({ ...window });
    return { window: copy(), client: copy(), layout: copy() };
  }

  // ---- model -------------------------------------------------------------------------------------------------

  getScrollPos(): number {
    return this.state.get().pos;
  }
  /** `SetScrollPos`: stores the value as given (the original does not clamp). */
  setScrollPos(pos: number): void {
    if (this.state.patch({ pos })) this.onScroll?.(pos);
  }
  getMinScrollPos(): number {
    return this.state.get().min;
  }
  getMaxScrollPos(): number {
    return this.state.get().max;
  }
  getScrollRange(): number {
    const s = this.state.get();
    return s.max - s.min;
  }
  setScrollRange(min: number, max: number): void {
    this.state.patch({ min, max });
  }
  getScrollPage(): number {
    return this.state.get().page;
  }
  setScrollPage(page: number): void {
    this.state.patch({ page });
  }
  setMinScrollPos(): void {
    this.setScrollPos(this.state.get().min);
  }
  /** `SetMaxScrollPos`: `max - page`, at least 0. */
  setMaxScrollPos(): void {
    const s = this.state.get();
    this.setScrollPos(Math.max(0, s.max - s.page));
  }
  /** `SetScrollDown`: one line down, never past `max - page`. */
  setScrollDown(): void {
    const s = this.state.get();
    this.setScrollPos(stepPos(1, s.pos, s.min, s.max, s.page));
  }

  /** The thumb in bar coordinates (`null` when it is not shown). */
  thumb(): { top: number; height: number; track: number } | null {
    const s = this.state.get();
    return thumbGeometry(
      rectHeight(this.rectWindowValue),
      s.min,
      s.max,
      s.page,
      s.pos,
      this.#arrow(),
    );
  }

  #arrow(): number {
    return Math.min(SCROLLBAR_WIDTH, rectWidth(this.rectWindowValue)) || SCROLLBAR_WIDTH;
  }

  // ---- parts -------------------------------------------------------------------------------------------------

  /** Which part of the bar a point (bar coordinates) is over. */
  partAt(pt: Point): ScrollPart | null {
    const w = rectWidth(this.rectWindowValue);
    const h = rectHeight(this.rectWindowValue);
    if (pt.x < 0 || pt.x >= w || pt.y < 0 || pt.y >= h) return null;
    const a = this.#arrow();
    if (pt.y < a) return 'up';
    // the lower arrow sits at `floor(h / 4) * 4 - a` (OnSize rounds the bottom down to a multiple of 4)
    const bottom = Math.floor(h / 4) * 4;
    if (pt.y >= bottom - a && pt.y < bottom) return 'down';
    if (pt.y >= h - a) return null; // the strip under the lower arrow (h is not a multiple of 4)
    const t = this.thumb();
    if (t && pt.y >= t.top && pt.y < t.top + t.height) return 'thumb';
    return 'track';
  }

  // ---- pointer -----------------------------------------------------------------------------------------------

  override onMouseMove(pt: Point): void {
    if (this.#dragging) {
      const s = this.state.get();
      const h = rectHeight(this.rectWindowValue);
      this.setScrollPos(posFromDrag(pt.y, this.#grab, h, s.min, s.max, s.page, this.#arrow()));
      return;
    }
    this.state.patch({ hover: this.partAt(pt) });
  }
  override onMouseEnter(): void {
    // the position arrives with the first move
  }
  override onMouseLeave(): void {
    this.state.patch({ hover: null });
  }

  override onLButtonDown(pt: Point): void {
    const part = this.partAt(pt);
    if (part === 'up' || part === 'down') {
      this.state.patch({ pressed: part });
      this.env.host.sound?.click(); // the arrows are buttons: every press plays SND_INF_CLICK
      return;
    }
    const s = this.state.get();
    if (s.page >= s.max) return; // `GetScrollPage() >= GetMaxScrollPos()`: nothing to scroll (VER15)
    if (part === 'thumb') {
      const t = this.thumb();
      if (!t) return;
      this.setCapture();
      this.#grab = pt.y - t.top;
      this.#dragging = true;
      this.state.patch({ pressed: 'thumb' });
    } else if (part === 'track' && this.getScrollRange() !== 0) {
      const t = this.thumb();
      if (t && pt.y < t.top) this.setScrollPos(pagePos(-1, s.pos, s.min, s.max, s.page));
      else if (t && pt.y > t.top + t.height) {
        this.setScrollPos(pagePos(1, s.pos, s.min, s.max, s.page));
      }
    }
  }
  override onLButtonDblClk(pt: Point): void {
    this.onLButtonDown(pt); // `OnLButtonDblClk` = `OnLButtonDown`
  }
  override onLButtonUp(pt: Point): void {
    const pressed = this.state.get().pressed;
    if (this.#dragging) {
      this.#dragging = false;
      this.releaseCapture();
    }
    if ((pressed === 'up' || pressed === 'down') && this.partAt(pt) === pressed) {
      const s = this.state.get();
      this.setScrollPos(stepPos(pressed === 'up' ? -1 : 1, s.pos, s.min, s.max, s.page));
    }
    this.state.patch({ pressed: null });
  }
  override onPointerRelease(): void {
    if (this.#dragging) {
      this.#dragging = false;
      this.releaseCapture();
    }
    this.state.patch({ pressed: null });
  }
  override onMouseWheel(notches: number): void {
    this.forwardWheel?.(notches);
  }
}

/** The rect of an owner's scroll bar in the owner's window coordinates (`rect.left = rect.right - 15`). */
export function ownerScrollBarRect(owner: WndBase): Rect {
  const w = rectWidth(owner.rectWindow);
  const h = rectHeight(owner.rectWindow);
  return { l: w - SCROLLBAR_WIDTH, t: 0, r: w, b: h };
}

/** Creates the docking scroll bar of `owner` (id 1000), hidden until the owner has `WBS_VSCROLL`. */
export function createOwnerScrollBar(owner: WndBase): ScrollBar {
  const bar = new ScrollBar();
  bar.create(WBS_CHILD | WBS_DOCKING, ownerScrollBarRect(owner), owner, SCROLLBAR_ID);
  return bar;
}
