// `WndBase`: the controller node of the UI (port of `CWndBase`, WndBase.cpp/.h; Plan/42 §4.1, ref/22 §3-§5).
//
// A window has three rects in PARENT-relative integer pixels (window, client, layout; §3.4), a style, a parent and
// children in creation order. Frames (`WndNeuz`) are children of the root and live in the window manager's z-order;
// controls are `WBS_CHILD` windows of a frame and are positioned relative to the frame's CLIENT rect (docking
// children relative to its WINDOW rect). Behaviour is mechanical: the virtual hooks below are the C++ virtuals.

import type { Shortcut } from '../dnd/shortcut';
import {
  centerIn,
  type FrameRects,
  frameRects,
  type Point,
  type Rect,
  rectCopy,
  rectHeight,
  rectOffset,
  rectWidth,
} from '../layout/geometry';
import {
  WBS_CAPTION,
  WBS_CHILD,
  WBS_DOCKING,
  WBS_NOFOCUS,
  WBS_POPUP,
  hasStyle,
} from '../layout/styles';
import type { WndEnv } from './env';
import type { WndNeuz } from './WndNeuz';

/** Modifier state and button of a pointer event delivered to a window hook. */
export interface PointerInfo {
  /** 0 left, 1 middle, 2 right. */
  button: number;
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
}

/** Modifier state of a key event delivered to a window hook. */
export interface KeyInfo {
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
}

/** `m_nToolTipPos`: 0 top (default), 1 bottom, 2 left, 3 right (Plan/42 §9.1, `CToolTip::Paint`). */
export type ToolTipPos = 0 | 1 | 2 | 3;

export abstract class WndBase {
  /** `m_nIdWnd`: the APP id of a frame, the WIDC id of a control. */
  id = 0;
  /** `m_dwStyle`: `WBS_*` bits plus the low bits of the window kind. */
  style = 0;
  parent: WndBase | null = null;
  /** `m_pWndFocusChild`: the child that has the focus inside this window. */
  focusChild: WndBase | null = null;
  /** `m_strToolTip` / `m_nToolTipPos`. */
  toolTip = '';
  toolTipPos: ToolTipPos = 0;
  /** A process-unique id (React keys, debugging); assigned by `create()`. */
  uid = 0;
  /** Set once the window was removed from its parent (`destroy` ran). */
  destroyed = false;
  /** `m_bTabStop` / `m_bDefault` (WP-UI-06): Tab cycles the tab stops of a `WBS_KEY` frame, Enter presses the default. */
  tabStop = false;
  isDefault = false;

  /** `m_wndArray`: children (controls, title bar, tab pages) in creation order; root: the frames too. */
  protected readonly wndArray: WndBase[] = [];
  protected rectWindowValue: Rect = { l: 0, t: 0, r: 0, b: 0 };
  protected rectClientValue: Rect = { l: 0, t: 0, r: 0, b: 0 };
  protected rectLayoutValue: Rect = { l: 0, t: 0, r: 0, b: 0 };
  protected visibleValue = true;
  protected enabledValue = true;
  #env: WndEnv | null = null;

  // ---- environment -------------------------------------------------------------------------------------------

  /** The shared environment; throws for a window that was never created under a parent. */
  get env(): WndEnv {
    if (!this.#env) throw new Error(`window ${this.constructor.name} is not created yet`);
    return this.#env;
  }

  /** `true` once `create()` ran (or the window is the root). */
  get isCreated(): boolean {
    return this.#env !== null;
  }

  /** `true` only for the root window (`m_pWndRoot`), which overrides the field. */
  readonly isRoot: boolean = false;

  // ---- rects (Plan/42 §3.4) ----------------------------------------------------------------------------------

  get rectWindow(): Rect {
    return this.rectWindowValue;
  }
  get rectClient(): Rect {
    return this.rectClientValue;
  }
  get rectLayout(): Rect {
    return this.rectLayoutValue;
  }

  /**
   * `CWndBase::SetWndRect`: stores the window rect and recomputes client and layout; when `onSize` is set and the
   * client SIZE changed, `onSize()` runs. Controls override `computeRects` (their `SetWndRect` overrides).
   */
  setWndRect(rect: Rect, onSize = false): void {
    const oldClient = this.rectClientValue;
    const r = this.computeRects(rect);
    this.rectWindowValue = r.window;
    this.rectClientValue = r.client;
    this.rectLayoutValue = r.layout;
    this.onRectChanged();
    if (
      onSize &&
      (rectWidth(oldClient) !== rectWidth(r.client) ||
        rectHeight(oldClient) !== rectHeight(r.client))
    ) {
      this.onSize(0, rectWidth(r.client), rectHeight(r.client));
    }
  }

  /** The client and layout rect for a window rect (`frameRects`); controls with their own rule override this. */
  protected computeRects(window: Rect): FrameRects {
    return frameRects(window, this.style, this.isRoot);
  }

  /** Called after the rects were replaced (frames publish their state, controls their view state). */
  protected onRectChanged(): void {
    // frames publish their state, controls their view state
  }

  /** `SetWndSize`: keeps the top-left, changes the size; runs `onSize` like the original (`SetWndRect(m_rectWindow)`). */
  setWndSize(w: number, h: number): void {
    const r = this.rectWindowValue;
    this.setWndRect({ l: r.l, t: r.t, r: r.l + w, b: r.t + h }, true);
  }

  /** `Move`: the window rect keeps its size and gets the new top-left (parent coordinates). */
  move(pt: Point): void {
    const r = this.rectWindowValue;
    this.setWndRect({ l: pt.x, t: pt.y, r: pt.x + rectWidth(r), b: pt.y + rectHeight(r) });
  }

  /** `MoveRectCenter`: centres the window in `area` (integer division like the original). */
  moveRectCenter(area: Rect): void {
    const r = this.rectWindowValue;
    this.setWndRect(centerIn(area, rectWidth(r), rectHeight(r)));
  }

  /** `MoveParentCenter`: centres in the ROOT window rect (the whole viewport, taskbar included; WndBase.cpp:1805). */
  moveParentCenter(): void {
    this.moveRectCenter(this.env.wm.root.rectWindow);
  }

  /** `MovePointCenter`: puts the window centre on `pt`. */
  movePointCenter(pt: Point): void {
    const r = this.rectWindowValue;
    const w = rectWidth(r);
    const h = rectHeight(r);
    const x = pt.x - Math.trunc(w / 2);
    const y = pt.y - Math.trunc(h / 2);
    this.setWndRect({ l: x, t: y, r: x + w, b: y + h });
  }

  /** `GetWindowRect`: parent-relative, or `(0, 0, w, h)` with `parentRel = false`. */
  getWindowRect(parentRel = true): Rect {
    const r = this.rectWindowValue;
    return parentRel ? rectCopy(r) : rectOffset(r, -r.l, -r.t);
  }

  /** `GetClientRect`: parent-relative (same space as the window rect), or `(0, 0, w, h)`. */
  getClientRect(parentRel = true): Rect {
    const c = this.rectClientValue;
    return parentRel ? rectCopy(c) : rectOffset(c, -c.l, -c.t);
  }

  /** `GetLayoutRect`: parent-relative, or relative to the client top-left. */
  getLayoutRect(parentRel = true): Rect {
    const l = this.rectLayoutValue;
    const c = this.rectClientValue;
    return parentRel ? rectCopy(l) : rectOffset(l, -c.l, -c.t);
  }

  /**
   * `GetScreenRect`: the window rect in root coordinates. Child windows add their parent's client top-left;
   * `WBS_DOCKING` children (title bar, scroll bars) add the parent's WINDOW top-left.
   */
  getScreenRect(): Rect {
    let rect = this.rectWindowValue;
    let style = this.style;
    let p = this.parent;
    while (p && !this.isRoot && hasStyle(style, WBS_CHILD)) {
      const o = hasStyle(style, WBS_DOCKING) ? p.rectWindowValue : p.rectClientValue;
      rect = rectOffset(rect, o.l, o.t);
      style = p.style;
      p = p.parent;
    }
    return rect;
  }

  /** The client rect in root coordinates (what `m_rectCurrentClient` holds during dispatch). */
  getScreenClientRect(): Rect {
    const s = this.getScreenRect();
    const w = this.rectWindowValue;
    const c = this.rectClientValue;
    return rectOffset(c, s.l - w.l, s.t - w.t);
  }

  /** `ScreenToWindow`: a root point relative to this window's top-left. */
  screenToWindow(pt: Point): Point {
    const s = this.getScreenRect();
    return { x: pt.x - s.l, y: pt.y - s.t };
  }

  // ---- style, visibility, state ------------------------------------------------------------------------------

  /** `IsWndStyle`: true when ANY of the bits is set. */
  isWndStyle(bits: number): boolean {
    return (this.style & bits) !== 0;
  }
  addWndStyle(bits: number): void {
    this.style |= bits;
  }
  delWndStyle(bits: number): void {
    this.style &= ~bits;
  }

  setVisible(v: boolean): void {
    if (this.visibleValue === v) return;
    this.visibleValue = v;
    this.onVisibleChanged();
  }
  isVisible(): boolean {
    return this.visibleValue;
  }
  enableWindow(on: boolean): void {
    if (this.enabledValue === on) return;
    this.enabledValue = on;
    this.onEnabledChanged();
  }
  isWindowEnabled(): boolean {
    return this.enabledValue;
  }
  protected onVisibleChanged(): void {
    // subclasses publish the new state
  }
  protected onEnabledChanged(): void {
    // subclasses publish the new state
  }

  setToolTip(text: string, pos: ToolTipPos = 0): void {
    this.toolTip = text;
    this.toolTipPos = pos;
  }

  /** `SetTabStop`: the window takes part in the Tab cycle of its `WBS_KEY` frame. */
  setTabStop(on: boolean): void {
    this.tabStop = on;
  }
  /** `SetDefault`: Enter presses this window in a `WBS_KEY` frame. */
  setDefault(on: boolean): void {
    this.isDefault = on;
  }

  // ---- tree --------------------------------------------------------------------------------------------------

  /** The children in creation order (a copy-free read-only view). */
  get childWnds(): readonly WndBase[] {
    return this.wndArray;
  }

  /** `GetFrameWnd`: this window when it is a frame, else the nearest ancestor that is not a `WBS_CHILD` window. */
  getFrameWnd(): WndNeuz | null {
    if (this.isRoot) return null;
    if (!hasStyle(this.style, WBS_CHILD)) return this as unknown as WndNeuz;
    let p = this.parent;
    while (p) {
      if (!hasStyle(p.style, WBS_CHILD)) return p as WndNeuz;
      p = p.parent;
    }
    return null;
  }

  /** `GetChildWnd(id)`: the first child with that window id. */
  getChildWnd(id: number): WndBase | null {
    return this.wndArray.find((w) => w.id === id) ?? null;
  }

  /**
   * `CWndBase::Create` (WndBase.cpp:297-357): ORs the style in, adopts the environment of the parent, computes the
   * rects, registers with the parent (frames enter the window manager's z-order and take the focus), then runs
   * `onInitialUpdate()` and `onSize()`.
   */
  create(style: number, rect: Rect, parent: WndBase, id: number): boolean {
    this.style |= style;
    this.visibleValue = true;
    this.id = id;
    this.parent = parent;
    this.#env = parent.env;
    this.uid = this.#env.wm.nextUid();
    this.setWndRect(rect, false);
    parent.addWnd(this);
    this.onInitialUpdate();
    this.onSize(0, rectWidth(this.rectClientValue), rectHeight(this.rectClientValue));
    return true;
  }

  /** The root window adopts its environment here (it has no parent). */
  protected adoptEnv(env: WndEnv): void {
    this.#env = env;
  }

  /** `CWndBase::AddWnd`: registers `w` with this window (children) or the window manager (frames). */
  addWnd(w: WndBase): void {
    if (w !== this) this.wndArray.push(w);
    if (!hasStyle(w.style, WBS_CHILD)) {
      this.env.wm.insertFrame(w as WndNeuz);
    } else if (!this.getFrameWnd()?.isWndStyle(WBS_NOFOCUS)) {
      w.setFocus();
    }
  }

  /** `CWndBase::RemoveWnd` for a child window. */
  removeWnd(w: WndBase): void {
    const i = this.wndArray.indexOf(w);
    if (i < 0) return;
    this.wndArray.splice(i, 1);
    w.onKillFocus(null);
    if (this.focusChild === w) {
      this.focusChild = null;
      const last = this.wndArray[this.wndArray.length - 1];
      last?.setFocus();
    }
  }

  // ---- focus -------------------------------------------------------------------------------------------------

  /** `SetFocus`: frames go to the top of their layer, children become the frame's focus child (§5.1). */
  setFocus(): void {
    this.env.wm.setFocusTo(this);
  }

  /** `true` when this is the focused leaf (`m_pCurFocus`). */
  isFocusWnd(): boolean {
    return this.env.wm.curFocus === this;
  }

  // ---- pointer capture ---------------------------------------------------------------------------------------

  setCapture(): void {
    this.env.wm.setCapture(this);
  }
  releaseCapture(): void {
    this.env.wm.releaseCapture(this);
  }

  // ---- hit testing -------------------------------------------------------------------------------------------

  /**
   * `IsPickupSpace(ptWindow)`: whether a point (relative to this window's top-left) belongs to it for hit-testing and
   * window moves. Captioned frames: the top 20 px; everything else: the whole window rect.
   */
  isPickupSpace(ptWindow: Point): boolean {
    const r = this.getWindowRect(false);
    const bottom = this.isWndStyle(WBS_CAPTION) ? 20 : r.b;
    return ptWindow.x >= r.l && ptWindow.x < r.r && ptWindow.y >= r.t && ptWindow.y < bottom;
  }

  // ---- virtual hooks (the C++ virtuals) ----------------------------------------------------------------------

  /** `OnInitialUpdate`: subclasses create their children here. */
  onInitialUpdate(): void {
    // no-op in the base class
  }
  /** `OnSize(type, cx, cy)`: the CLIENT size changed. */
  onSize(type: number, cx: number, cy: number): void;
  onSize(): void {
    // no-op in the base class
  }
  /** `OnDestroy`: runs once, before the window is removed. */
  onDestroy(): void {
    // no-op in the base class
  }
  /** `OnSetFocus(pOldWnd)`. */
  onSetFocus(old: WndBase | null): void;
  onSetFocus(): void {
    // no-op in the base class
  }
  /**
   * `OnKillFocus(pNewWnd)`: releases the capture; a `WBS_POPUP` window hides when it loses the focus
   * (menus close when the focus moves, WndBase.cpp:2331-2340).
   */
  onKillFocus(newWnd: WndBase | null): void;
  onKillFocus(): void {
    if (this.env.wm.captureOwner === this) this.releaseCapture();
    if (this.isWndStyle(WBS_POPUP)) this.setVisible(false);
  }
  /** `OnChildNotify(message, id, src)`: returns `true` when handled. */
  onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  onChildNotify(): boolean {
    return false;
  }
  /** `OnCommand(id, message, source)`: returns `true` when handled. */
  onCommand(id: number, msg: number, src?: WndBase): boolean;
  onCommand(): boolean {
    return false;
  }
  /** `OnDropIcon(shortcut, framePoint)`: return `true` when the drop was accepted. */
  onDropIcon?(sc: Shortcut, pt: Point): boolean;

  /** `OnLButtonDown(point)`: `point` is relative to the CLIENT top-left. */
  onLButtonDown?(pt: Point, info: PointerInfo): void;
  onLButtonUp?(pt: Point, info: PointerInfo): void;
  onLButtonDblClk?(pt: Point, info: PointerInfo): void;
  onRButtonDown?(pt: Point, info: PointerInfo): void;
  onRButtonUp?(pt: Point, info: PointerInfo): void;
  onMouseMove?(pt: Point, info: PointerInfo): void;
  /** The pointer entered / left this window (hover; buttons highlight on it). */
  onMouseEnter?(): void;
  onMouseLeave?(): void;
  /** `OnMouseWheel(delta)`: `notches` > 0 scrolls up, one notch = 1. */
  onMouseWheel?(notches: number, pt: Point): void;
  /**
   * The mouse press that started on this window ended (a release anywhere, or a pointer cancel): the C++ clears
   * `m_bLButtonDown`/`m_bPush` of every window on `WM_LBUTTONUP`; a button un-pushes here even when the pointer
   * was released outside of it.
   */
  onPointerRelease?(): void;
  /**
   * Space or Enter pressed this window in a `WBS_KEY` frame (`DefWindowProc(WM_KEYDOWN)` calls `OnLButtonDown` of the
   * default or focused child with `m_bKeyButton` set) and released it (`WM_KEYUP` -> `OnLButtonUp`).
   */
  onKeyPress?(): void;
  onKeyRelease?(): void;
  /** `OnKeyDown`/`OnKeyUp` of the focused window and its frame (`KeyboardEvent.code`; shells override them). */
  onKeyDown?(code: string, info: KeyInfo): void;
  onKeyUp?(code: string, info: KeyInfo): void;

  /** `Process`: periodic update; frames call it at `UI_PROCESS_HZ` (§4.3). The base runs the children's. */
  process(now: number): void {
    for (const c of this.wndArray) c.process(now);
  }
}
