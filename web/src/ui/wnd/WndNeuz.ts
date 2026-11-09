// `WndNeuz`: the frame controller every game window extends (port of `CWndNeuz`, WndNeuz.cpp; Plan/42 §4, §5.11, §7).
//
// A shell class sets `appId`, calls `initialize(parent)` (-> `initDialog`) and ports `onInitialUpdate` (control
// setup after `super.onInitialUpdate()`), `onChildNotify` / `onCommand` and `process`. The base creates the title
// bar, instantiates the layout controls in file order, rounds tiled frames up to 16 px and bakes the skin.

import {
  type Point,
  type Rect,
  rectEq,
  rectFromTuple,
  rectHeight,
  rectWidth,
  rectXYWH,
  roundUp16,
} from '../layout/geometry';
import type { AppKey, AppletLayout, ControlLayout, WidcKey } from '../layout/registry';
import {
  type ControlKind,
  WBS_CAPTION,
  WBS_CHILD,
  WBS_DOCKING,
  WBS_HELP,
  WBS_NOFRAME,
  WBS_NODRAWFRAME,
  WTBID_CLOSE,
  WTBID_HELP,
  WTYPE_CUSTOM,
  WTYPE_STATIC,
  WID_TITLE_BAR,
  Wnm,
} from '../layout/styles';
import { APP_LOGIN } from '../../gen/defs/resdata';
import { type BakedSkin, type BakeSpec, frameSkinOps, type SkinOp } from '../skin/baker';
import type { Shortcut } from '../dnd/shortcut';
import { Control } from './Control';
import { ControlState } from './observable';
import { TitleBar } from './TitleBar';
import { WndBase } from './WndBase';

/** A `WTYPE_CUSTOM` placeholder that the controller turned into a canvas (§8.11). */
export interface CustomSlot {
  /** `WIDC_*` name of the placeholder. */
  name: string;
  /** The placeholder rect in the frame's client coordinates. */
  rect: Rect;
  canvas: HTMLCanvasElement;
}

/** The state of a frame as the React view renders it; a new object whenever one field changed. */
export interface FrameState {
  /** The window rect (parent = root coordinates). */
  rect: Rect;
  /** The client rect relative to the window top-left (where the controls' coordinate origin sits). */
  clientOffset: Point;
  title: string;
  visible: boolean;
  enabled: boolean;
  /** The baked skin canvas contents; `null` until the first bake finished. */
  skin: BakedSkin | null;
  /** Bumps whenever a new skin was published (the view repaints its canvas on it). */
  skinVersion: number;
  /** The instantiated controls in creation order. */
  controls: readonly Control[];
  customs: readonly CustomSlot[];
  titleBar: TitleBar | null;
  /** `true` for a frame without skin texture: the title is drawn at (17, 7) by the theme fallback. */
  untextured: boolean;
  /**
   * The alpha (0..255) of the skin and the title of THIS frame instead of the global `windowAlpha` (`null` / absent = the global one). The quest quick-info window fades its frame with the pointer
   * (`m_nAlphaCount`, task ui-quest); the text and the controls stay opaque.
   */
  skinAlpha?: number | null;
}

export abstract class WndNeuz extends WndBase {
  /** The `ResData.h` APP id of the window. */
  abstract readonly appId: number;

  /** Observable state of the frame view. */
  readonly view: ControlState<FrameState>;
  /** `m_bNoCloseButton`: no CLOSE button in the title bar (decided before `initialize`). */
  noCloseButton = false;
  /**
   * Set by shells that need `process()` every frame instead of every 100 ms (§2.4 rule 2):
   * `override readonly processEveryFrame = true;`. (`declare`: no class field is emitted here, so a subclass
   * getter or field is not shadowed.)
   */
  declare readonly processEveryFrame?: boolean | undefined;
  /** Multi-instance windows (`APP_MESSAGE` per partner): the key of the instance. */
  declare readonly instanceKey?: string | undefined;
  /**
   * `IsPutRegInfo()`: `false` keeps the window out of the position persistence (`wm/regInfo.ts`, §17). Applet
   * shells that the original excluded (the tutorial and a few dialogs) set it before `initialize`.
   */
  putRegInfo = true;
  /** The format version of what `serializeRegInfo` returns (stored as `extraVersion`, handed to `restoreRegInfo`). */
  regInfoVersion = 1;
  /**
   * The DOM element of the window lets the pointer through (`pointer-events: none`). A window that covers the world view but claims
   * only a few points by `isPickupSpace` (the buff strip) must not stop the pointer events of the world canvas below it: the window
   * manager listens at the document and picks by `isPickupSpace`, the canvas listens for itself (found by the M4 demo: the strip
   * over the whole view took every click of the world).
   */
  domPassthrough = false;
  /** `m_bTile` / `m_strTexture` of the applet. */
  tile = false;
  texture = '';
  /**
   * `AdjustMinRect`/`AdjustMaxRect` hook of resizable (`WBS_THICKFRAME`) frames: receives the rect a resize drag
   * produced and the current rect, returns the rect to apply (chat, navigator and quest quick info define one).
   */
  constrainResize?(next: Rect, current: Rect): Rect;
  /** Extra frame hook after the controls' skins were painted (chat edit backgrounds, ...). */
  afterSkin?: BakeSpec['after'];
  afterSkinImages?: BakeSpec['afterImages'];
  /** The layout this window was created from (`null` until `onInitialUpdate`). */
  protected applet: AppletLayout | null = null;
  protected titleBar: TitleBar | null = null;
  /** Controls instantiated from the layout or by `createControl`, in creation order. */
  protected readonly controlList: Control[] = [];

  #title = '';
  #skinVersion = 0;
  #bakeDirty = false;
  #baking = false;
  #bakeWaiters: (() => void)[] = [];
  readonly #customSlots: CustomSlot[] = [];
  readonly #dirtyCustoms = new Set<string>();
  #initParent: WndBase | null = null;

  constructor() {
    super();
    this.view = new ControlState<FrameState>({
      rect: { l: 0, t: 0, r: 0, b: 0 },
      clientOffset: { x: 0, y: 0 },
      title: '',
      visible: true,
      enabled: true,
      skin: null,
      skinVersion: 0,
      controls: [],
      customs: [],
      titleBar: null,
      untextured: false,
    });
  }

  // ---- creation (Plan/42 §4.2) -------------------------------------------------------------------------------

  /** `Initialize(pParent)` -> `InitDialog(appId)`: the shell's entry point. */
  initialize(parent: WndBase, extraStyle = 0): boolean {
    return this.initDialog(this.appId, extraStyle, { x: 0, y: 0 }, parent);
  }

  /**
   * `CWndNeuz::InitDialog(hWnd, APP_X, style, pt, parent)` (WndNeuz.cpp:659-678): looks the applet up (an unknown
   * layout returns `false`: the window is not created), builds the rect from the layout size (`APP_LOGIN` height
   * forced to 200), ORs the layout style with `extraStyle` (+ `WBS_HELP` for a non-empty help key) and `create`s it.
   */
  protected initDialog(
    app: AppKey,
    extraStyle = 0,
    pt: Point = { x: 0, y: 0 },
    parent?: WndBase,
  ): boolean {
    const p = parent ?? this.#initParent;
    if (!p) {
      throw new Error(`${this.constructor.name}.initDialog needs a parent window (wm.root)`);
    }
    const layout = p.env.registry.resolve(app);
    if (!layout) {
      p.env.log.error('ui', `no window layout for ${String(app)}`);
      return false;
    }
    const h = layout.id === APP_LOGIN ? 200 : layout.h;
    const rect = rectXYWH(pt.x, pt.y, layout.w, h);
    let style = layout.style | extraStyle;
    if (layout.help !== '') style |= WBS_HELP;
    this.#initParent = p;
    return this.create(style, rect, p, layout.id);
  }

  /**
   * `CWndNeuz::OnInitialUpdate` (WndNeuz.cpp:191-239): the title bar for `WBS_CAPTION`, then the applet's title,
   * tile flag and texture, then every control in file order (`WTYPE_CUSTOM` creates nothing). Subclasses call
   * `super.onInitialUpdate()` first.
   */
  override onInitialUpdate(): void {
    super.onInitialUpdate();
    if (this.isWndStyle(WBS_CAPTION)) {
      const bar = new TitleBar();
      bar.create(
        WBS_CHILD | WBS_DOCKING | WBS_NODRAWFRAME | WBS_NOFRAME,
        { l: 0, t: 0, r: 0, b: 0 },
        this,
        WID_TITLE_BAR,
      );
      this.titleBar = bar;
      bar.replace();
    }
    const applet = this.env.registry.byId(this.id) ?? null;
    this.applet = applet;
    if (applet) {
      this.setTitle(applet.title);
      this.tile = applet.tile;
      this.texture = applet.texture;
      for (const c of applet.controls) this.createLayoutControl(c);
    }
    this.publish();
  }

  /** `CreateControl` for one layout row; returns `null` for placeholders and unsupported kinds. */
  protected createLayoutControl(c: ControlLayout): Control | null {
    if (c.type === WTYPE_CUSTOM) return null;
    const ctl = this.env.controls(c.type, c);
    if (!ctl) return null;
    // `CWndStatic::Create` adds WBS_CAPTION | WBS_CHILD | WBS_NOFRAME; every other control gets WBS_CHILD
    let style = c.style | WBS_CHILD;
    if (c.type === WTYPE_STATIC) style |= WBS_CAPTION | WBS_NOFRAME;
    ctl.create(style, rectFromTuple(c.rect), this, c.id);
    this.controlList.push(ctl);
    return ctl;
  }

  /** `createControl(kind, id, rect, style)`: a control created by code (no layout row). */
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- see getDlgItem
  createControl<T extends Control = Control>(
    kind: ControlKind,
    id: number,
    rect: Rect,
    style = 0,
  ): T {
    const ctl = this.env.controls(kind, null);
    if (!ctl) throw new Error(`no control class for WTYPE ${String(kind)}`);
    ctl.create(style | WBS_CHILD, rect, this, id);
    this.controlList.push(ctl);
    this.publish();
    return ctl as T;
  }

  /**
   * Attaches a control object that the shell built itself (a `Gauge`, a `Menu`, a subclass with extra state) as a
   * child created by code: the same steps as `createControl`.
   */
  addControl<T extends Control>(ctl: T, id: number, rect: Rect, style = 0): T {
    ctl.create(style | WBS_CHILD, rect, this, id);
    this.controlList.push(ctl);
    this.publish();
    return ctl;
  }

  /** Removes a control created by `createControl`/`addControl` (menu items, tree check boxes): it is gone for good. */
  removeControl(ctl: Control): void {
    const i = this.controlList.indexOf(ctl);
    if (i >= 0) this.controlList.splice(i, 1);
    this.removeWnd(ctl);
    ctl.destroyed = true;
    this.publish();
  }

  // ---- lookup ------------------------------------------------------------------------------------------------

  /** `GetDlgItem(WIDC_X)`: the instantiated control with that id or name; `null` when absent. */
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- the porting API of Plan/42 §4.6: callers name the control class
  getDlgItem<T extends Control = Control>(id: WidcKey): T | null {
    if (typeof id === 'string') return this.getDlgItemByName<T>(id);
    return (this.controlList.find((c) => c.id === id) as T | undefined) ?? null;
  }

  /** `GetDlgItem` by `WIDC_*` name (the four undefined names are reachable this way only). */
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- see getDlgItem
  getDlgItemByName<T extends Control = Control>(name: string): T | null {
    return (this.controlList.find((c) => c.name === name) as T | undefined) ?? null;
  }

  /**
   * `GetWndCtrl(WIDC_X)->rect`: the LAYOUT rect of a control (also of `WTYPE_CUSTOM` placeholders), a copy in
   * client coordinates; it does not follow later moves of the control.
   */
  getWndCtrl(id: WidcKey): { rect: Rect; layout: ControlLayout } | null {
    if (!this.applet) return null;
    const layout = this.env.registry.control(this.applet, id);
    return layout ? { rect: rectFromTuple(layout.rect), layout } : null;
  }

  /** The controls in creation order (read-only). */
  get controls(): readonly Control[] {
    return this.controlList;
  }

  // ---- title -------------------------------------------------------------------------------------------------

  setTitle(text: string): void {
    this.#title = text;
    this.view.patch({ title: text });
  }
  getTitle(): string {
    return this.#title;
  }

  /** The alpha of the skin and the title of this frame (`null` = the global `windowAlpha`); see `FrameState.skinAlpha`. */
  setSkinAlpha(alpha: number | null): void {
    this.view.patch({ skinAlpha: alpha });
  }

  // ---- size, rects, skin -------------------------------------------------------------------------------------

  /**
   * `CWndNeuz::OnSize` (WndNeuz.cpp:493-512): a tiled frame is rounded UP to a multiple of 16 in both directions,
   * then the skin is (re)baked and the title bar replaced.
   */
  override onSize(type: number, cx: number, cy: number): void {
    if (this.isRoot) return;
    if (this.tile) {
      const r = this.rectWindowValue;
      const rounded = rectXYWH(r.l, r.t, roundUp16(rectWidth(r)), roundUp16(rectHeight(r)));
      if (!rectEq(r, rounded)) this.setWndRect(rounded, false);
    }
    this.requestBake();
    this.titleBar?.replace();
    super.onSize(type, cx, cy);
  }

  protected override onRectChanged(): void {
    const o = {
      x: this.rectClientValue.l - this.rectWindowValue.l,
      y: this.rectClientValue.t - this.rectWindowValue.t,
    };
    const cur = this.view.get();
    if (
      !rectEq(cur.rect, this.rectWindowValue) ||
      cur.clientOffset.x !== o.x ||
      cur.clientOffset.y !== o.y
    ) {
      this.view.patch({ rect: { ...this.rectWindowValue }, clientOffset: o });
    }
  }
  protected override onVisibleChanged(): void {
    this.view.patch({ visible: this.visibleValue });
    // a hidden modal must stop blocking, a shown one must start (the manager's snapshot names the modal frame)
    if (this.isCreated) this.env.wm.notifyVisibility();
  }
  protected override onEnabledChanged(): void {
    this.view.patch({ enabled: this.enabledValue });
  }

  /** The ops and image list of the skin right now (`AdjustWndBase`); `null` when nothing is baked (§7.4). */
  buildBakeSpec(): BakeSpec | null {
    const r = this.rectWindowValue;
    const f = frameSkinOps({
      style: this.style,
      texture: this.texture,
      tile: this.tile,
      width: rectWidth(r),
      height: rectHeight(r),
      visible: this.visibleValue,
    });
    if (!f) return null;
    const ops: SkinOp[] = [...f.ops];
    if (f.childrenBaked) {
      for (const c of this.wndArray) {
        if (c.destroyed || !c.isVisible()) continue;
        if (c instanceof Control) ops.push(...c.skinOps());
      }
    }
    const first = f.ops[0];
    const size =
      first?.op === 'frame'
        ? { width: first.width, height: first.height }
        : { width: rectWidth(r), height: rectHeight(r) };
    // an empty window (a popup menu before its first item) has nothing to paint
    if (size.width <= 0 || size.height <= 0) return null;
    return {
      ...size,
      ops,
      ...(this.afterSkin ? { after: this.afterSkin } : {}),
      ...(this.afterSkinImages ? { afterImages: this.afterSkinImages } : {}),
    };
  }

  /** Schedules a (re)bake: at creation, on a size change and when a skin input changed. Coalesced. */
  requestBake(): void {
    if (!this.isCreated || this.destroyed) return;
    this.#bakeDirty = true;
    if (!this.#baking) void this.#runBake();
  }

  /** Resolves when no bake is pending or running (tests, screenshot waits). */
  whenBaked(): Promise<void> {
    if (!this.#baking && !this.#bakeDirty) return Promise.resolve();
    return new Promise((resolve) => this.#bakeWaiters.push(resolve));
  }

  async #runBake(): Promise<void> {
    this.#baking = true;
    try {
      while (this.#bakeDirty && !this.destroyed) {
        this.#bakeDirty = false;
        const spec = this.buildBakeSpec();
        if (!spec) {
          this.#publishSkin(null);
          continue;
        }
        const skin = await this.env.baker.bake(spec);
        if (this.#isGone()) break;
        if (!this.#isBakeDirty()) this.#publishSkin(skin);
      }
    } catch (e) {
      this.env.log.error('ui', `skin bake of ${String(this.appId)} failed: ${String(e)}`);
    } finally {
      this.#baking = false;
      const waiters = this.#bakeWaiters;
      this.#bakeWaiters = [];
      for (const w of waiters) w();
    }
  }

  /** Method reads: the fields change across the `await` of a running bake (no stale narrowing). */
  #isBakeDirty(): boolean {
    return this.#bakeDirty;
  }
  #isGone(): boolean {
    return this.destroyed;
  }

  #publishSkin(skin: BakedSkin | null): void {
    this.#skinVersion += 1;
    this.view.patch({
      skin,
      skinVersion: this.#skinVersion,
      untextured: this.texture === '',
    });
  }

  /** Publishes the lists (controls, custom canvases, title bar) to the view. */
  protected publish(): void {
    this.view.patch({
      controls: [...this.controlList],
      customs: [...this.#customSlots],
      titleBar: this.titleBar,
      untextured: this.texture === '',
    });
  }

  // ---- custom canvases (§8.11) -------------------------------------------------------------------------------

  /**
   * A `<canvas>` placed at the rect of a `WTYPE_CUSTOM` placeholder; created on first use, sized for the device
   * pixel ratio and mounted by the frame view. `null` outside a browser or for an unknown placeholder.
   */
  customCanvas(id: WidcKey): HTMLCanvasElement | null {
    if (typeof document === 'undefined') return null;
    const c = this.getWndCtrl(id);
    if (!c) return null;
    return this.#slotCanvas(c.layout.name, c.rect, true);
  }

  /**
   * A `<canvas>` at an arbitrary client rect, not at a layout placeholder: the 3-D viewports of the character select
   * and create windows of the original sit at hard-coded rects (`WndTitle.cpp` `m_aRect`, `(280, 0, 550, 320)`). One
   * canvas per `name`; `scale` multiplies the backing store (1 = the CSS size, e.g. a preview that is rendered at
   * its CSS size and copied pixel for pixel).
   */
  addCustomCanvas(name: string, rect: Rect, deviceScale = false): HTMLCanvasElement | null {
    if (typeof document === 'undefined') return null;
    return this.#slotCanvas(name, rect, deviceScale);
  }

  #slotCanvas(name: string, rect: Rect, deviceScale: boolean): HTMLCanvasElement {
    const existing = this.#customSlots.find((s) => s.name === name);
    if (existing) return existing.canvas;
    const canvas = document.createElement('canvas');
    const dpr =
      deviceScale && typeof devicePixelRatio === 'number' && devicePixelRatio > 0
        ? devicePixelRatio
        : 1;
    const w = rectWidth(rect);
    const h = rectHeight(rect);
    canvas.width = Math.max(1, Math.round(w * dpr));
    canvas.height = Math.max(1, Math.round(h * dpr));
    canvas.style.width = `${String(w)}px`;
    canvas.style.height = `${String(h)}px`;
    this.#customSlots.push({ name, rect: { ...rect }, canvas });
    this.publish();
    return canvas;
  }

  /** Removes a canvas made by `addCustomCanvas` / `customCanvas` (the preview of a deleted character). */
  removeCustomCanvas(name: string): void {
    const i = this.#customSlots.findIndex((s) => s.name === name);
    if (i < 0) return;
    this.#customSlots.splice(i, 1);
    this.#dirtyCustoms.delete(name);
    this.publish();
  }

  /** Schedules `onDraw` for one placeholder (or all of them); the draws run in the next UI tick. */
  invalidate(id?: WidcKey): void {
    if (id === undefined) {
      for (const s of this.#customSlots) this.#dirtyCustoms.add(s.name);
    } else {
      const c = this.getWndCtrl(id);
      if (c) this.#dirtyCustoms.add(c.layout.name);
    }
    if (this.#dirtyCustoms.size > 0) this.env.wm.markDrawDirty(this);
  }

  /** Runs the pending `onDraw` calls (the window manager calls it at the end of every tick). */
  flushDraws(): void {
    if (this.#dirtyCustoms.size === 0) return;
    const names = [...this.#dirtyCustoms];
    this.#dirtyCustoms.clear();
    if (!this.onDraw) return;
    for (const name of names) {
      const slot = this.#customSlots.find((s) => s.name === name);
      const ctx = slot?.canvas.getContext('2d');
      if (slot && ctx) {
        const dpr = slot.canvas.width / Math.max(1, rectWidth(slot.rect));
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, rectWidth(slot.rect), rectHeight(slot.rect));
        this.onDraw(name as WidcKey, ctx);
      }
    }
  }

  /** Custom painting of a placeholder (§8.11); only called for canvases requested through `customCanvas`. */
  onDraw?(id: WidcKey, ctx: CanvasRenderingContext2D): void;

  // ---- notifications (Plan/42 §4.4) --------------------------------------------------------------------------

  /**
   * Base handling of the title-bar buttons: CLOSE destroys the frame, HELP opens the help window (M8; a no-op
   * before). Shells override and call `super.onChildNotify` for the rest.
   */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WTBID_CLOSE) {
        this.destroy();
        return true;
      }
      if (id === WTBID_HELP) return true;
    }
    return false;
  }

  // ---- life cycle --------------------------------------------------------------------------------------------

  /** Deferred destroy (§5.9): hides and disables at once, the manager removes the frame at the next tick. */
  destroy(): void {
    this.env.wm.destroy(this);
  }

  /** `OnDestroy` of the base: custom canvases are released. */
  override onDestroy(): void {
    // the controls clean up after themselves (a combo box destroys its popup frame)
    for (const c of this.controlList) {
      try {
        c.onDestroy();
      } catch (e) {
        this.env.log.error(
          'ui',
          `onDestroy of a control of ${String(this.appId)} failed: ${String(e)}`,
        );
      }
    }
    this.#customSlots.length = 0;
    this.#dirtyCustoms.clear();
    super.onDestroy();
  }

  /** Per-window persistence of extra state (§17); the base keeps none. */
  serializeRegInfo?(): unknown;
  restoreRegInfo?(v: unknown, version: number): void;
}
