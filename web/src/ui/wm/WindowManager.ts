// The window manager core (port of the root window `CWndMgr`/`CWndBase` root logic; Plan/42 §5, ref/22 §4.3, §5):
// z-order layers, focus, modal blocking, pointer dispatch, move/snap/clamp, THICKFRAME resize, singleton/instance
// map, deferred destroy, viewport alignment and the UI tick. It is DOM-free: the pointer methods take UI virtual
// pixel coordinates, `UiRoot.tsx` translates browser events into them and `FrameWindowView` renders the order.

import {
  APP_BUFF_STATUS,
  APP_COMMUNICATION_CHAT,
  APP_GH_MAIN,
  APP_MINIGAME_DICE,
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  APP_COLLECTING,
  APP_GUILDCOMBAT_SELSTART,
  APP_NAVIGATOR,
  APP_QUEITMWARNING,
  APP_RAINBOWRACE_BUTTON,
  APP_REVIVAL,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_LADDER,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
  APP_SKILL3,
  APP_MESSAGE_TEXT,
  APP_STATE_CONFIRM,
  APP_STATUS1,
  APP_VENDOREX_BUY,
  APP_VENDOREX_SELL,
  APP_VENDOR_REVISION,
} from '../../gen/defs/resdata';
import type { Logger } from '../../platform/log';
import { defaultControlFactory, type ControlFactory } from '../controls/factory';
import { DragController } from '../dnd/DragController';
import type { Shortcut } from '../dnd/shortcut';
import type { UiHost } from '../host';
import {
  type Point,
  type Rect,
  rectContains,
  rectCopy,
  rectWidth,
  rectHeight,
  rectXYWH,
} from '../layout/geometry';
import type { LayoutRegistry } from '../layout/registry';
import {
  WBS_CHILD,
  WBS_KEY,
  WBS_MANAGER,
  WBS_MODAL,
  WBS_MOVE,
  WBS_NOFOCUS,
  WBS_POPUP,
  WBS_THICKFRAME,
  WBS_TOPMOST,
  WM_LBUTTONDBLCLK,
  WM_LBUTTONUP,
  WM_RBUTTONUP,
} from '../layout/styles';
import type { SkinBaker } from '../skin/baker';
import type { UiImageCache } from '../skin/images';
import type { WndEnv } from '../wnd/env';
import type { KeyInfo, PointerInfo } from '../wnd/WndBase';
import { WndBase } from '../wnd/WndBase';
import type { WndNeuz } from '../wnd/WndNeuz';
import type { WindowManagerApi } from './api';
import {
  type AppletHooks,
  AppletRegistry,
  objectExecutorApplet,
  openApplet,
  openField,
} from './applets';
import { resizeDir, resizeRect, snapMove } from './dragMath';

/** Windows' default double-click time (`GetDoubleClickTime`) and half-box (`SM_CXDOUBLECLK` / 2). */
export const DOUBLE_CLICK_MS = 500;
export const DOUBLE_CLICK_PX = 2;

/** `UI_PROCESS_HZ`: how often `process()` runs for a live frame (Plan/42 §4.3). */
export const UI_PROCESS_HZ = 10;

/** Applets that the Esc chain never closes (`CWndBase::GetWndBase()`, WndBase.cpp:2048-2100; Plan/42 §5.7). */
export const ESC_SKIP_APPS: readonly number[] = [
  APP_STATUS1,
  APP_NAVIGATOR,
  APP_REVIVAL,
  APP_BUFF_STATUS,
  APP_RAINBOWRACE_BUTTON,
  APP_VENDOR_REVISION,
  // the gate choice of the siege has no title bar and no way out but its own timer (WP-UI-49)
  APP_GUILDCOMBAT_SELSTART,
];

/** Windows that the Esc close step finds but does NOT destroy (mini-games, the guild house menu). */
export const ESC_KEEP_APPS: readonly number[] = [
  APP_MINIGAME_KAWIBAWIBO,
  APP_MINIGAME_KAWIBAWIBO_WIN,
  APP_MINIGAME_DICE,
  APP_COLLECTING,
  APP_RR_MINIGAME_ARITHMETIC,
  APP_RR_MINIGAME_CARD,
  APP_RR_MINIGAME_DICE,
  APP_RR_MINIGAME_KAWIBAWIBO,
  APP_RR_MINIGAME_LADDER,
  APP_RR_MINIGAME_STOPWATCH,
  APP_RR_MINIGAME_TYPING,
  APP_GH_MAIN,
];

/** The root window (`CWndMgr` as a window): the virtual viewport; its layout rect excludes the taskbar. */
export class RootWnd extends WndBase {
  #layout: Rect = { l: 0, t: 0, r: 0, b: 0 };

  constructor(env: WndEnv) {
    super();
    this.adoptEnv(env);
    this.style = WBS_MANAGER;
    this.id = 0;
  }

  override readonly isRoot = true;

  /** Root: client = window; layout = the area windows may occupy (viewport minus the taskbar). */
  protected override computeRects(window: Rect): {
    window: Rect;
    client: Rect;
    layout: Rect;
  } {
    return { window: rectCopy(window), client: rectCopy(window), layout: this.#layout };
  }

  /** Sets viewport and layout rect together. */
  setViewport(window: Rect, layout: Rect): void {
    this.#layout = rectCopy(layout);
    this.setWndRect(window);
  }
}

/** What `pointerDown/Move/Up/wheel` report to the caller (43's world input, the DOM glue). */
export interface PointerResult {
  /** The event belongs to the UI: the world must not see it (a window was hit, a drag runs, a shown modal has the focus). */
  consumed: boolean;
  /** The frame under the pointer (`null` over the world). */
  frame: WndNeuz | null;
  /** The deepest window under the pointer (`frame`, one of its controls, or the root). */
  target: WndBase | null;
}

/** One row of the window rect registry (`__flyffDebug.ui.order()` of Plan/42 §21.2). */
export interface WindowRectInfo {
  app: number;
  key: string;
  /** The window rect in root (UI virtual) coordinates. */
  rect: Rect;
  /** The z-index of the frame (`index * 10`). */
  z: number;
  visible: boolean;
  focused: boolean;
}

/** The immutable state React renders; replaced whenever the order, the focus or the modal changes. */
export interface WmSnapshot {
  readonly version: number;
  /** Frames bottom -> top. */
  readonly order: readonly WndNeuz[];
  readonly focusFrame: WndBase;
  /** The topmost visible `WBS_MODAL` frame (its blocker element sits just below it), if any. */
  readonly modal: WndNeuz | null;
  readonly viewport: Rect;
}

export type WindowManagerEvent =
  | { type: 'pointerDown'; pt: Point }
  | { type: 'opened'; frame: WndNeuz }
  | { type: 'closed'; frame: WndNeuz }
  | { type: 'focus'; frame: WndBase }
  /** The deepest window under the pointer changed (tooltips, cursors); `null` = nothing (the world). */
  | { type: 'hover'; target: WndBase | null }
  /** The pointer moved to `pt` (virtual pixels); emitted after the hover change and before the hooks (tooltips, cursors). */
  | { type: 'pointerMove'; pt: Point }
  /** The virtual viewport changed (the taskbar re-lays out, `setViewport` already aligned the windows). */
  | { type: 'viewport'; viewport: Rect }
  /** A pointer release is being dispatched (emitted first: an armed drag ends here, WP-UI-12). */
  | { type: 'pointerUp'; pt: Point }
  /** The UI tick ran (`now` in ms; the drag controller expires the forbid icon on it). */
  | { type: 'tick'; now: number };

/**
 * The consumer of the shortcut drag (`DragController`, WP-UI-12): told how a drop ended. `target` is the window under
 * the pointer (`null` over the world, i.e. no UI element), `accepted` the result of its `onDropIcon`.
 */
export interface DragSink {
  /** The button was released while a shortcut was dragged; the manager clears the drag after this call. */
  dropped(sc: Shortcut, target: WndBase | null, accepted: boolean, pt: Point): void;
  /** The pointer left the UI / was cancelled while a shortcut was dragged (no drop). */
  cancelled(sc: Shortcut): void;
}

/** Hooks of the skill-tree window for the Esc chain (`GetWndBase()` special case, Plan/42 §5.7). */
export interface SkillTreeEscHook {
  /** `GetCurrSkillPoint() != player.skillPoint`: unspent edits. */
  hasUnspentEdits(): boolean;
  /** Opens the re-skill warning window. */
  openWarning(): void;
}

/** The part of the persistence layer the manager needs (`wm/regInfo.ts`, WP-UI-09). */
export interface WindowRectStore {
  restore(frame: WndNeuz): void;
  save(frame: WndNeuz): void;
}

export interface WindowManagerOptions {
  registry: LayoutRegistry;
  images: UiImageCache;
  baker: SkinBaker;
  host: UiHost;
  log: Logger;
  controls?: ControlFactory;
  /** Initial viewport; `taskbarHeight` is subtracted from the layout rect. */
  viewport?: { width: number; height: number; taskbarHeight?: number };
}

interface DragState {
  kind: 'move' | 'resize';
  frame: WndNeuz;
  /** Move: the pointer offset inside the window; resize: the pointer position at the start. */
  grab: Point;
  dir: number;
  rectOld: Rect;
}

const instanceKeyOf = (app: number, key: string): string => `${String(app)}|${key}`;

export class WindowManager implements WindowManagerApi {
  readonly root: RootWnd;
  readonly env: WndEnv;
  /** The applet table and the factories the window shells register (§12, WP-UI-15). */
  readonly applets: AppletRegistry;
  /** The collaborators of the applet rules that live outside the manager (logout box, world map, taskbar, ...). */
  appletHooks: AppletHooks = {};
  /** Optional collaborators set by later work packages. */
  rectStore: WindowRectStore | null = null;
  skillTreeEsc: SkillTreeEscHook | null = null;
  /** Probe: does a UI `<input>`/`<textarea>` have DOM focus? (`isEditFocused`) */
  editFocusProbe: () => boolean = defaultEditFocusProbe;
  /** Set by the drag controller (WP-UI-12): receives the end of every shortcut drag. */
  dragSink: DragSink | null = null;
  /** The drag-and-drop core (`ui.dnd`, §10): arming, the drag layer's state, the forbid icon, the world-drop handler. */
  readonly drag: DragController;

  #order: WndNeuz[] = [];
  #focusFrame: WndBase;
  #curFocus: WndBase;
  #capture: WndBase | null = null;
  #drag: DragState | null = null;
  #pressed: WndBase | null = null;
  /** The window held down by Space/Enter in a `WBS_KEY` frame (`m_bKeyButton`). */
  #keyButton: WndBase | null = null;
  #hover: WndBase | null = null;
  #lastPointer: Point | null = null;
  #lastDown: { at: number; pt: Point; button: number } | null = null;
  #shortcut: Shortcut | null = null;
  #tempModal: { frame: WndNeuz; previous: WndBase } | null = null;
  readonly #instances = new Map<string, WndNeuz>();
  readonly #factories = new Map<number, () => WndNeuz>();
  readonly #removeQueue: WndNeuz[] = [];
  readonly #lastProcess = new Map<WndNeuz, number>();
  readonly #keys = new WeakMap<WndNeuz, string>();
  readonly #drawDirty = new Set<WndNeuz>();
  readonly #hidden: WndNeuz[] = [];
  readonly #listeners = new Set<() => void>();
  readonly #eventListeners = new Set<(e: WindowManagerEvent) => void>();
  #clearFlag = false;
  /** `setFieldScope`: is the session in the world right now? (frames inserted meanwhile belong to the field) */
  #fieldScope: (() => boolean) | null = null;
  /** The frames inserted while the session was in the world (or while `closeField` ran): `closeField` destroys them. */
  readonly #fieldFrames = new WeakSet<WndNeuz>();
  #closingField = false;
  #uid = 0;
  #snapshot: WmSnapshot;
  #version = 0;
  #viewport: Rect = { l: 0, t: 0, r: 0, b: 0 };
  #layout: Rect = { l: 0, t: 0, r: 0, b: 0 };
  readonly #log: Logger;

  constructor(options: WindowManagerOptions) {
    this.#log = options.log;
    this.env = {
      wm: this,
      registry: options.registry,
      images: options.images,
      baker: options.baker,
      controls: options.controls ?? defaultControlFactory,
      host: options.host,
      log: options.log,
    };
    this.drag = new DragController({ wm: this, now: () => options.host.now() });
    this.drag.attach();
    this.root = new RootWnd(this.env);
    this.applets = new AppletRegistry(this, {
      log: options.log,
      ...(options.host.features ? { features: () => options.host.features } : {}),
    });
    this.#focusFrame = this.root;
    this.#curFocus = this.root;
    const vp = options.viewport ?? { width: 1280, height: 720, taskbarHeight: 0 };
    this.#applyViewport(vp.width, vp.height, vp.taskbarHeight ?? 0);
    this.#snapshot = this.#makeSnapshot();
  }

  // ---- viewport ----------------------------------------------------------------------------------------------

  /** The root window rect: the whole virtual viewport (including the taskbar). */
  get viewport(): Rect {
    return this.#viewport;
  }
  /** `m_pWndRoot->GetLayoutRect()`: where windows may be (the viewport minus the taskbar); the move/resize clamp. */
  get layoutRect(): Rect {
    return this.#layout;
  }

  /** Sets the virtual viewport and re-aligns the windows (Plan/42 §5.10, §16). */
  setViewport(width: number, height: number, taskbarHeight = 0): void {
    const oldLayout = this.#layout;
    this.#applyViewport(width, height, taskbarHeight);
    this.alignAll(oldLayout, this.#layout);
    this.#bump();
    this.#emit({ type: 'viewport', viewport: this.#viewport });
  }

  #applyViewport(width: number, height: number, taskbarHeight: number): void {
    this.#viewport = rectXYWH(0, 0, width, height);
    this.#layout = rectXYWH(0, 0, width, Math.max(0, height - taskbarHeight));
    this.root.setViewport(this.#viewport, this.#layout);
  }

  // ---- React snapshot ----------------------------------------------------------------------------------------

  /** `useSyncExternalStore` subscription. */
  readonly subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };
  /** The current immutable snapshot (stable until something changes). */
  readonly getSnapshot = (): WmSnapshot => this.#snapshot;

  /** Subscribes to window events (opened, closed, pointer down, focus). Returns the unsubscribe function. */
  onEvent(fn: (e: WindowManagerEvent) => void): () => void {
    this.#eventListeners.add(fn);
    return () => {
      this.#eventListeners.delete(fn);
    };
  }
  #emit(e: WindowManagerEvent): void {
    for (const fn of [...this.#eventListeners]) {
      try {
        fn(e);
      } catch (err) {
        this.#log.error('ui', `window event listener failed: ${String(err)}`);
      }
    }
  }

  #makeSnapshot(): WmSnapshot {
    let modal: WndNeuz | null = null;
    for (let i = this.#order.length - 1; i >= 0; i -= 1) {
      const f = this.#order[i];
      if (f && f.isVisible() && f.isWndStyle(WBS_MODAL)) {
        modal = f;
        break;
      }
    }
    return {
      version: this.#version,
      order: this.#order,
      focusFrame: this.#focusFrame,
      modal,
      viewport: this.#viewport,
    };
  }

  #bump(): void {
    this.#version += 1;
    this.#order = [...this.#order];
    this.#snapshot = this.#makeSnapshot();
    for (const fn of [...this.#listeners]) fn();
  }

  /** Window visibility changed (a hidden modal must stop blocking, ...). */
  notifyVisibility(): void {
    this.#bump();
  }

  nextUid(): number {
    this.#uid += 1;
    return this.#uid;
  }

  // ---- order (Plan/42 §5.1) ----------------------------------------------------------------------------------

  /** The frames bottom -> top (the root is conceptually index 0 and not listed). */
  order(): readonly WndNeuz[] {
    return this.#order;
  }

  /**
   * `CWndBase::AddWnd` for a non-child window: `WBS_MANAGER` frames go below the first non-manager; TOPMOST, MODAL
   * and POPUP frames are appended (top layer); every other frame goes before the first VISIBLE
   * topmost/popup/modal one. Unless `WBS_NOFOCUS` the new frame takes the focus.
   */
  insertFrame(frame: WndNeuz): void {
    if (this.#closingField || this.#fieldScope?.() === true) this.#fieldFrames.add(frame);
    const order = this.#order;
    if (frame.isWndStyle(WBS_MANAGER)) {
      let i = 0;
      while (i < order.length && order[i]?.isWndStyle(WBS_MANAGER)) i += 1;
      order.splice(i, 0, frame);
    } else if (order.length > 0) {
      if (frame.isWndStyle(WBS_TOPMOST | WBS_MODAL | WBS_POPUP)) {
        order.push(frame);
      } else {
        let i = 0;
        while (i < order.length) {
          const w = order[i];
          if (w && w.isWndStyle(WBS_TOPMOST | WBS_POPUP | WBS_MODAL) && w.isVisible()) break;
          i += 1;
        }
        order.splice(i, 0, frame);
      }
    } else {
      order.push(frame);
    }
    this.#bump();
    if (!frame.isWndStyle(WBS_NOFOCUS)) frame.setFocus();
  }

  /** `CWndBase::RemoveWnd` on the root: the frame leaves the order; losing the focus hands it to the root. */
  removeFrame(frame: WndNeuz): void {
    const i = this.#order.indexOf(frame);
    if (i >= 0) this.#order.splice(i, 1);
    const k = this.root.childWnds.indexOf(frame);
    if (k >= 0) this.root.removeWnd(frame);
    if (this.#hover && this.#belongsTo(this.#hover, frame)) this.#hover = null;
    if (this.#pressed && this.#belongsTo(this.#pressed, frame)) this.#pressed = null;
    if (this.#capture && this.#belongsTo(this.#capture, frame)) this.#capture = null;
    if (this.#drag?.frame === frame) this.#drag = null;
    if (this.#focusFrame === frame) this.root.setFocus();
    this.#bump();
  }

  #belongsTo(w: WndBase, frame: WndNeuz): boolean {
    return w === frame || w.getFrameWnd() === frame;
  }

  // ---- focus (Plan/42 §5.1) ----------------------------------------------------------------------------------

  /** `m_pWndFocus`: the focused frame (or the root). */
  get focusFrame(): WndBase {
    return this.#focusFrame;
  }
  /** `m_pCurFocus`: the focused leaf (the frame itself, one of its controls, or the root). */
  get curFocus(): WndBase {
    return this.#curFocus;
  }

  /**
   * `CWndBase::SetFocus` (WndBase.cpp:1632-1691). A child becomes its parent's focus child and the frame's current
   * focus. A frame moves to the top of its layer first (before the first visible topmost/popup/modal window above
   * it; a popup does not stay above another popup), then replaces the focused frame, whose focus chain is killed
   * (popups hide).
   */
  setFocusTo(w: WndBase): void {
    if (w.isWndStyle(WBS_CHILD) && w.parent) {
      const p = w.parent;
      if (p.focusChild !== w) {
        p.focusChild?.onKillFocus(w);
        const old = p.focusChild;
        p.focusChild = w;
        w.onSetFocus(old);
      } else {
        w.onSetFocus(w);
      }
      const previousFrame = this.#focusFrame;
      this.#focusFrame = w.getFrameWnd() ?? this.root;
      this.#curFocus = w;
      if (previousFrame !== this.#focusFrame) this.#bump();
      return;
    }
    const frame = w as WndNeuz;
    if (w !== this.root && !w.isWndStyle(WBS_MANAGER) && this.#order.length > 0) {
      this.#raise(frame);
    }
    if (this.#focusFrame !== w) {
      const oldFrame = this.#focusFrame;
      this.#killFocusChain(oldFrame, w);
      const oldChild = oldFrame.focusChild;
      if (w.focusChild) w.focusChild.onSetFocus(oldChild);
      this.#focusFrame = w;
      this.#curFocus = w;
      w.onSetFocus(oldFrame);
    } else {
      this.#curFocus = w;
    }
    this.#emit({ type: 'focus', frame: w });
    this.#bump();
  }

  #killFocusChain(old: WndBase, to: WndBase): void {
    old.onKillFocus(to);
    let c = old.focusChild;
    const seen = new Set<WndBase>();
    while (c && !seen.has(c)) {
      seen.add(c);
      c.onKillFocus(null);
      c = c.focusChild;
    }
  }

  /** Moves a visible frame to the top of its layer (the `SetFocus` re-insertion). */
  #raise(frame: WndNeuz): void {
    const order = this.#order;
    const i = order.indexOf(frame);
    if (i < 0 || !frame.isVisible()) return;
    order.splice(i, 1);
    let j = i;
    for (; j < order.length; j += 1) {
      const o = order[j];
      if (!o) continue;
      const blocks =
        o.isWndStyle(WBS_TOPMOST) ||
        (o.isWndStyle(WBS_POPUP) && !frame.isWndStyle(WBS_POPUP)) ||
        o.isWndStyle(WBS_MODAL);
      if (blocks && o.isVisible()) break;
    }
    order.splice(j, 0, frame);
  }

  // ---- capture, shortcuts -----------------------------------------------------------------------------------

  get captureOwner(): WndBase | null {
    return this.#capture;
  }
  setCapture(w: WndBase): void {
    this.#capture = w;
  }
  releaseCapture(w?: WndBase): void {
    if (!w || this.#capture === w) this.#capture = null;
  }

  /** The shortcut being dragged (`m_GlobalShortcut`); the drag controller of WP-UI-12 sets and clears it. */
  get draggedShortcut(): Shortcut | null {
    return this.#shortcut;
  }
  setDraggedShortcut(sc: Shortcut | null): void {
    this.#shortcut = sc;
  }
  isDraggingShortcut(): boolean {
    return this.#shortcut !== null;
  }

  // ---- applets (Plan/42 §12; WP-UI-15) ------------------------------------------------------------------------

  /** Open or focus an applet (`CreateApplet`); never toggles. */
  openApplet(app: number): void {
    openApplet(this, this.applets, app);
  }

  /** `ObjectExecutor(SHORTCUT_APPLET)`: the toggle policy of §12.2. */
  objectExecutorApplet(app: number): void {
    objectExecutorApplet(this, this.applets, this.appletHooks, app);
  }

  /** The world-entry sequence of §12.4. */
  openField(): void {
    openField(this, this.applets, this.appletHooks);
  }

  /** The start menu above the taskbar (§11.4; the taskbar service installs the opener). */
  openStartMenu(aboveTaskbar: true): void;
  openStartMenu(): void {
    this.appletHooks.openStartMenu?.();
  }

  // ---- lifecycle / singletons (Plan/42 §5.9) ----------------------------------------------------------------

  /** Registers the factory of an applet (`AddAppletFunc`); `open(app)` without `build` uses it. */
  registerFactory(app: number, factory: () => WndNeuz): void {
    this.#factories.set(app, factory);
  }

  /**
   * `CreateApplet`: an existing instance (same app and key) is focused and returned; otherwise the factory builds
   * the window, `initialize(root)` runs, the stored rect is applied and the instance is recorded. A window whose
   * layout does not exist (`initialize` returns `false`) is never recorded.
   */
  open<T extends WndNeuz>(app: number, build?: () => T, key = ''): T | null {
    const k = instanceKeyOf(app, key);
    const existing = this.#instances.get(k);
    if (existing && !existing.destroyed) {
      existing.setFocus();
      return existing as T;
    }
    const factory = build ?? this.#factories.get(app);
    if (!factory) {
      this.#log.warn('ui', `no factory for window ${String(app)}`);
      return null;
    }
    const w = factory();
    let created: boolean;
    try {
      created = w.initialize(this.root);
    } catch (e) {
      // a shell that throws while it builds itself must not leave a half-built frame in the order
      this.#log.error('ui', `window ${String(app)} failed to initialise: ${String(e)}`);
      if (w.isCreated) {
        markDestroyed(w);
        this.removeFrame(w);
      }
      return null;
    }
    if (!created) return null;
    this.#instances.set(k, w);
    this.#keys.set(w, key);
    this.rectStore?.restore(w);
    this.#emit({ type: 'opened', frame: w });
    return w as T;
  }

  /** The live instance of an app (`GetWndBase(APP)` / `GetApplet`). */
  get(app: number, key = ''): WndNeuz | null {
    const w = this.#instances.get(instanceKeyOf(app, key));
    return w && !w.destroyed ? w : null;
  }

  /**
   * Gives the live instance of (`app`, `from`) the key `to` (a renamed IM partner, `CWndMgr::UpdateMessage`): the window keeps its place in the order and
   * `get( app, to )` finds it from now on. `false` when there is no such window or another one already holds `to`.
   */
  rekey(app: number, from: string, to: string): boolean {
    const old = instanceKeyOf(app, from);
    const w = this.#instances.get(old);
    if (!w || w.destroyed) return false;
    if (from === to) return true;
    const target = instanceKeyOf(app, to);
    const other = this.#instances.get(target);
    if (other && other !== w && !other.destroyed) return false;
    this.#instances.delete(old);
    this.#instances.set(target, w);
    this.#keys.set(w, to);
    return true;
  }

  /** `true` while any instance of the app exists (and is not being destroyed). */
  isOpen(app: number): boolean {
    for (const w of this.#instances.values()) {
      if (w.appId === app && !w.destroyed) return true;
    }
    return false;
  }

  /** Closes (destroys) the instance of an app. */
  close(app: number, key = ''): void {
    const w = this.get(app, key);
    if (w) this.destroy(w);
  }

  /**
   * `CWndBase::Destroy`: queues the removal and hides and disables the frame at once; `onDestroy`, the removal from
   * the order and the singleton map happen at the start of the next tick (or at the end of the current pointer
   * event), so handlers can still read a frame they just closed.
   */
  destroy(w: WndNeuz): void {
    if (w.destroyed || this.#removeQueue.includes(w)) return;
    this.#removeQueue.push(w);
    w.enableWindow(false);
    w.setVisible(false);
    this.#bump();
  }

  /** Whether a frame waits for its removal (`IsDestroy`). */
  isDestroying(w: WndNeuz): boolean {
    return this.#removeQueue.includes(w);
  }

  /** `RemoveDestroyWnd`: runs the queued removals. */
  flushDestroyed(): void {
    if (this.#removeQueue.length === 0) return;
    const queue = this.#removeQueue.splice(0);
    for (const w of queue) {
      try {
        this.rectStore?.save(w);
        w.onDestroy();
      } catch (e) {
        this.#log.error('ui', `onDestroy of ${String(w.appId)} failed: ${String(e)}`);
      }
      markDestroyed(w);
      this.removeFrame(w);
      for (const [k, inst] of this.#instances) {
        if (inst === w) this.#instances.delete(k);
      }
      this.#lastProcess.delete(w);
      this.#drawDirty.delete(w);
      this.#emit({ type: 'closed', frame: w });
    }
  }

  // ---- the field scope (BFIX1 #021) --------------------------------------------------------------------------

  /**
   * Installs the question "is the session in the world?" (`FIELD` screens: Loading, InGame, Reconnecting, Leaving).
   * Every top-level frame inserted while it answers `true` is a FIELD frame that {@link closeField} destroys; `null`
   * (the default, the UI gallery and the tests) tags nothing.
   */
  setFieldScope(fn: (() => boolean) | null): void {
    this.#fieldScope = fn;
  }

  /**
   * The counterpart of `CWndMgr::OpenTitle(FALSE)` = `DestroyApplet()` + `Free()` (WndManager.cpp:2129-2139,
   * :1013-1035): destroys every field frame (hidden frames, popups, menus, message boxes, the taskbar and the buff
   * strip included) and flushes at once, so every applet rect goes through `rectStore.save` now, while the character
   * is still known. A shortcut drag, a window move/resize and the hover state end first (`pointerCancel`). Frames that
   * `onDestroy` hooks open meanwhile are tagged and closed too (a bounded loop); the numpad-`*` state and a temporary
   * modal chat are dropped. Idempotent; returns the number of frames that were closed.
   */
  closeField(): number {
    this.pointerCancel();
    const closed = new Set<WndNeuz>();
    this.#closingField = true;
    try {
      for (let pass = 0; pass < 16; pass += 1) {
        const live = this.#order.filter((f) => !f.destroyed && this.#fieldFrames.has(f));
        if (live.length === 0 && this.#removeQueue.length === 0) break;
        for (const f of live) {
          closed.add(f);
          this.destroy(f);
        }
        this.flushDestroyed();
      }
    } finally {
      this.#closingField = false;
    }
    this.#clearFlag = false;
    this.#hidden.length = 0;
    this.#tempModal = null;
    this.#bump();
    return closed.size;
  }

  // ---- tick (Plan/42 §4.3) -----------------------------------------------------------------------------------

  /**
   * The UI tick (called from the frame loop after rendering): runs the queued removals, then `process(now)` of every
   * live visible frame at most every `1000 / UI_PROCESS_HZ` ms (every call for `processEveryFrame` frames), then the
   * pending custom-canvas draws.
   */
  tick(now: number): void {
    this.#emit({ type: 'tick', now });
    this.flushDestroyed();
    const interval = 1000 / UI_PROCESS_HZ;
    for (const f of [...this.#order]) {
      if (f.destroyed || !f.isVisible()) continue;
      const last = this.#lastProcess.get(f);
      if (f.processEveryFrame === true || last === undefined || now - last >= interval) {
        this.#lastProcess.set(f, now);
        try {
          f.process(now);
        } catch (e) {
          this.#log.error('ui', `process of ${String(f.appId)} failed: ${String(e)}`);
        }
      }
    }
    if (this.#drawDirty.size > 0) {
      const dirty = [...this.#drawDirty];
      this.#drawDirty.clear();
      for (const f of dirty) {
        if (!f.destroyed) f.flushDraws();
      }
    }
  }

  /** A frame has custom canvases to repaint at the end of the tick. */
  markDrawDirty(frame: WndNeuz): void {
    this.#drawDirty.add(frame);
  }

  // ---- hit testing and pointer dispatch (Plan/42 §5.3) -------------------------------------------------------

  /**
   * `GetChildFocus`: the deepest descendant of `w` that claims the point. Children are scanned in creation order
   * and a later child overrides an earlier one ("later children win"), recursively; invisible and disabled
   * children are skipped; `isPickupSpace` decides whether a child claims the point.
   */
  childAt(w: WndBase, pt: Point): WndBase {
    let sel: WndBase = w;
    for (const c of w.childWnds) {
      if (!c.isWndStyle(WBS_CHILD) || !c.isVisible() || !c.isWindowEnabled()) continue;
      if (c.isPickupSpace(c.screenToWindow(pt))) sel = c;
      const deeper = this.childAt(c, pt);
      if (deeper !== c) sel = deeper;
    }
    return sel;
  }

  /** The visible frame under the point (top -> bottom) and its deepest claimed window. */
  hitTest(pt: Point): { frame: WndNeuz | null; target: WndBase } {
    for (let i = this.#order.length - 1; i >= 0; i -= 1) {
      const f = this.#order[i];
      if (!f?.isVisible() || !rectContains(f.getScreenRect(), pt)) continue;
      const target = this.childAt(f, pt);
      // a pass-through frame (the buff windows, `domPassthrough`) covers the world view and claims only the points its children or its own
      // `isPickupSpace` claim (the icons): the rest of its rect belongs to what lies below it, the world (a drop on the ground, a click
      // on a monster)
      if (f.domPassthrough && target === f && !f.isPickupSpace(f.screenToWindow(pt))) continue;
      return { frame: f, target };
    }
    return { frame: null, target: this.root };
  }

  /** `IsOpenModalWnd`: the focused frame has `WBS_MODAL`. */
  modalOpen(): boolean {
    return this.#focusFrame !== this.root && this.#focusFrame.isWndStyle(WBS_MODAL);
  }

  /**
   * The modal frame claims the whole screen for the pointer (the world gets no press, move or wheel, `PointerResult.consumed`): it is
   * focused AND shown. A modal that was just closed is hidden at once and keeps the focus until the next tick removes it
   * ({@link destroy}); it claims nothing in that frame (it has no blocker element either, `WmSnapshot.modal`).
   */
  #modalClaims(): boolean {
    return this.modalOpen() && this.#focusFrame.isVisible();
  }

  /**
   * Pointer press (`WindowRootProc` + `DefWindowProc`, ref/22 §5.2): cancels tooltips (event), applies the modal
   * rule (the focus never leaves a modal frame), focuses the frame and the child under the pointer, then hands the
   * press to the focused leaf: a hit inside its client rect -> `onLButtonDown`; a press on the THICKFRAME border
   * starts a resize; a press in the pick-up area of the focused `WBS_MOVE` frame starts a move.
   *
   * @param double `true` for the second press of a double click (`WM_LBUTTONDBLCLK`); when omitted the manager
   *   detects it like Windows does: the same button again within 500 ms and 4 px of the previous press (a press
   *   that completed a double click never starts the next one). DOM `pointerdown` events carry no click count.
   */
  pointerDown(pt: Point, info: PointerInfo, double?: boolean): PointerResult {
    const isDouble = double ?? this.#detectDouble(pt, info);
    this.#lastPointer = pt;
    this.#emit({ type: 'pointerDown', pt });
    const hit = this.#captureHit() ?? this.hitTest(pt);
    const modal = this.modalOpen();
    const claims = this.#modalClaims(); // before the flush below removes a modal that was just closed
    if (modal) {
      this.childAt(this.#focusFrame, pt).setFocus();
    } else {
      if (hit.frame) hit.frame.setFocus();
      else this.root.setFocus();
      if (hit.frame && hit.target !== hit.frame) hit.target.setFocus();
    }
    this.#defDown(this.#curFocus, pt, info, isDouble);
    this.flushDestroyed();
    return {
      consumed: hit.frame !== null || claims || this.#drag !== null,
      frame: hit.frame,
      target: hit.target,
    };
  }

  /** Windows' double-click rule (500 ms, a 4 x 4 px box) on the manager's clock (`host.now()`). */
  #detectDouble(pt: Point, info: PointerInfo): boolean {
    const now = this.env.host.now();
    const last = this.#lastDown;
    const double =
      last !== null &&
      last.button === info.button &&
      now - last.at <= DOUBLE_CLICK_MS &&
      Math.abs(pt.x - last.pt.x) <= DOUBLE_CLICK_PX &&
      Math.abs(pt.y - last.pt.y) <= DOUBLE_CLICK_PX;
    this.#lastDown = double ? null : { at: now, pt, button: info.button };
    return double;
  }

  #captureHit(): { frame: WndNeuz | null; target: WndBase } | null {
    const c = this.#capture;
    if (!c) return null;
    return { frame: c.getFrameWnd(), target: c };
  }

  /** `DefWindowProc(WM_LBUTTONDOWN / DBLCLK / RBUTTONDOWN)` for the focused leaf `w`. */
  #defDown(w: WndBase, pt: Point, info: PointerInfo, double: boolean): void {
    if (w === this.root) return;
    const sc = w.getScreenClientRect();
    const sw = w.getScreenRect();
    const ptClient = { x: pt.x - sc.l, y: pt.y - sc.t };
    const ptWindow = { x: pt.x - sw.l, y: pt.y - sw.t };
    if (info.button === 2) {
      if (rectContains(sc, pt) || this.#capture === w) {
        this.#pressed = w;
        w.onRButtonDown?.(ptClient, info);
      }
      return;
    }
    if (info.button !== 0) return;
    const inClient = rectContains(sc, pt) || this.#capture === w;
    const isFrame = !w.isWndStyle(WBS_CHILD);
    let grab = { x: pt.x - w.rectWindow.l, y: pt.y - w.rectWindow.t };
    let dir = 0;
    if (inClient) {
      this.#pressed = w;
      if (double) {
        w.onLButtonDblClk?.(ptClient, info);
        if (!isFrame && w.isWindowEnabled()) w.parent?.onCommand(w.id, WM_LBUTTONDBLCLK);
      } else {
        w.onLButtonDown?.(ptClient, info);
      }
    } else if (rectContains(sw, pt)) {
      this.#pressed = w;
      if (!double && w === this.#curFocus && w.isWndStyle(WBS_THICKFRAME)) {
        dir = resizeDir(ptWindow, rectWidth(w.rectWindow), rectHeight(w.rectWindow));
        if (dir !== 0) grab = { x: pt.x, y: pt.y };
      }
    }
    if (double || !isFrame) return;
    const frame = w as WndNeuz;
    if (dir !== 0) {
      this.#drag = { kind: 'resize', frame, grab, dir, rectOld: rectCopy(w.rectWindow) };
    } else if (this.#focusFrame === w && w.isWndStyle(WBS_MOVE) && w.isPickupSpace(ptWindow)) {
      this.#drag = { kind: 'move', frame, grab, dir: 0, rectOld: rectCopy(w.rectWindow) };
      this.setCapture(w);
    }
  }

  /**
   * Pointer move: an unchanged position is dropped; hover enter/leave go to the deepest window under the pointer
   * (the capture owner while a capture exists); a running move/resize applies; the window under the pointer gets
   * `onMouseMove`. A move/resize ends when the pointer leaves the root rect (VER15 safety, WndBase.cpp:1182-1195).
   */
  pointerMove(pt: Point, info: PointerInfo): PointerResult {
    const last = this.#lastPointer;
    if (last?.x === pt.x && last.y === pt.y) {
      const h = this.#hover;
      return {
        consumed: h !== null && h !== this.root,
        frame: h?.getFrameWnd() ?? null,
        target: h,
      };
    }
    this.#lastPointer = pt;
    const hit = this.#captureHit() ?? this.hitTest(pt);
    const target = this.#capture ?? hit.target;
    if (target !== this.#hover) {
      const prev = this.#hover;
      this.#hover = target;
      prev?.onMouseLeave?.();
      target.onMouseEnter?.();
      this.#emit({ type: 'hover', target: target === this.root ? null : target });
    }
    this.#emit({ type: 'pointerMove', pt });
    if (this.#drag) {
      if (pt.x < 0 || pt.y < 0 || pt.x > this.#viewport.r || pt.y > this.#viewport.b) {
        this.#endDrag();
      } else {
        this.#applyDrag(pt);
      }
    }
    if (target !== this.root) {
      const sc = target.getScreenClientRect();
      target.onMouseMove?.({ x: pt.x - sc.l, y: pt.y - sc.t }, info);
    }
    return {
      consumed:
        hit.frame !== null || this.#capture !== null || this.#drag !== null || this.#modalClaims(),
      frame: hit.frame,
      target,
    };
  }

  #applyDrag(pt: Point): void {
    const d = this.#drag;
    if (!d) return;
    const f = d.frame;
    if (d.kind === 'move') {
      const w = rectWidth(f.rectWindow);
      const h = rectHeight(f.rectWindow);
      const p = snapMove({ x: pt.x - d.grab.x, y: pt.y - d.grab.y }, w, h, this.#layout);
      f.setWndRect({ l: p.x, t: p.y, r: p.x + w, b: p.y + h }, true);
    } else {
      let r = resizeRect(
        d.dir,
        d.rectOld,
        { x: pt.x - d.grab.x, y: pt.y - d.grab.y },
        this.#layout,
      );
      r = f.constrainResize ? f.constrainResize(r, f.rectWindow) : r;
      f.setWndRect(r, true);
    }
  }

  #endDrag(): void {
    if (this.#drag?.kind === 'move' && this.#capture === this.#drag.frame) this.releaseCapture();
    this.#drag = null;
  }

  /**
   * Pointer release: a dragged shortcut is dropped on the window under the pointer (`OnDropIcon`; a drop on a popup
   * moves the focus to the menu's owner) and cleared; the focused leaf gets `onLButtonUp` and, when it is a child
   * that was pressed and the pointer is inside it, `parent.onCommand(id, WM_LBUTTONUP, child)`; a move or resize
   * ends and the capture is released.
   */
  pointerUp(pt: Point, info: PointerInfo): PointerResult {
    this.#lastPointer = pt;
    this.#emit({ type: 'pointerUp', pt });
    const hit = this.#captureHit() ?? this.hitTest(pt);
    if (info.button === 0 && this.#shortcut) {
      const sc = this.#shortcut;
      // the drop lands on the window UNDER the pointer, whoever holds the capture (the inventory captures for its paper doll)
      const dropHit = this.#capture === null ? hit : this.hitTest(pt);
      const target = dropHit.target !== this.root ? dropHit.target : null;
      let accepted = false;
      if (target) {
        const sw = target.getScreenRect();
        if (target.onDropIcon) {
          accepted = target.onDropIcon(sc, { x: pt.x - sw.l, y: pt.y - sw.t });
        } else {
          // `CWndBase::OnDropIcon`: a window without a drop handler refuses with the forbid icon
          this.drag.forbid();
        }
        if (target.isWndStyle(WBS_POPUP)) {
          let t: WndBase | null = target;
          while (t?.isWndStyle(WBS_POPUP) === true) t = t.parent;
          t?.parent?.setFocus();
        }
      }
      // the drag ends in every case (§10.3); the sink decides about the world drop and the forbid icon
      this.#shortcut = null;
      this.dragSink?.dropped(sc, target, accepted, pt);
    }
    const w = this.#curFocus;
    if (w !== this.root) {
      const sc = w.getScreenClientRect();
      const sw = w.getScreenRect();
      const ptClient = { x: pt.x - sc.l, y: pt.y - sc.t };
      const clickable = this.#pressed === w;
      if (info.button === 2) {
        if (rectContains(sc, pt) || this.#capture === w) {
          w.onRButtonUp?.(ptClient, info);
          if (w.isWndStyle(WBS_CHILD) && clickable && w.isWindowEnabled()) {
            w.parent?.onCommand(w.id, WM_RBUTTONUP, w);
          }
        }
      } else if (info.button === 0) {
        if (rectContains(sc, pt) || this.#capture === w) {
          w.onLButtonUp?.(ptClient, info);
          if (w.isWndStyle(WBS_CHILD) && clickable && w.isWindowEnabled() && rectContains(sw, pt)) {
            w.parent?.onCommand(w.id, WM_LBUTTONUP, w);
          }
        } else if (rectContains(sw, pt)) {
          w.onLButtonUp?.({ x: pt.x - sw.l, y: pt.y - sw.t }, info);
        }
      }
    }
    if (info.button === 0) {
      const pressed = this.#pressed;
      this.#pressed = null;
      this.#endDrag();
      pressed?.onPointerRelease?.(); // `WM_LBUTTONUP` clears `m_bPush` of the pressed window wherever the pointer is
    }
    this.flushDestroyed();
    return {
      consumed: hit.frame !== null || this.#modalClaims(),
      frame: hit.frame,
      target: hit.target,
    };
  }

  /**
   * Mouse wheel (owner #008, BFIX1): the wheel goes to the window UNDER the pointer, not to the focused leaf. From the
   * deepest window that claims the point (the capture owner while a capture exists) the manager walks up the parent
   * chain to the frame and calls the first visible, enabled window that has an `onMouseWheel` handler, with the point
   * relative to ITS client rect. The focus never changes (the chat edit keeps the keyboard). The wheel is consumed
   * whenever a frame is under the pointer or a modal is open, so the world never zooms below a window; while a modal
   * is open only the focused frame (the modal, or a popup it opened) is delivered to. DIFFERS from the C++
   * `DefWindowProc`, where only `m_pCurFocus` gets `WM_MOUSEWHEEL` (WndBase.cpp:1085-1104, :1467-1475; ref/22 §5.2).
   */
  wheel(pt: Point, notches: number): PointerResult {
    const hit = this.#captureHit() ?? this.hitTest(pt);
    const modal = this.modalOpen();
    let consumed = hit.frame !== null || this.#modalClaims();
    if (hit.frame !== null && (!modal || hit.frame === this.#focusFrame)) {
      for (let w: WndBase | null = hit.target; w !== null && w !== this.root; w = w.parent) {
        if (w.onMouseWheel && w.isVisible() && w.isWindowEnabled()) {
          const sc = w.getScreenClientRect();
          w.onMouseWheel(notches, { x: pt.x - sc.l, y: pt.y - sc.t });
          consumed = true;
          break;
        }
        if (w === hit.frame) break;
      }
    }
    return { consumed, frame: hit.frame, target: hit.target };
  }

  /** The pointer left the UI (window blur, pointer cancel): ends drags and clears hover/press state. */
  pointerCancel(): void {
    this.#endDrag();
    const pressed = this.#pressed;
    this.#pressed = null;
    const dragged = this.#shortcut;
    this.#shortcut = null;
    if (dragged) this.dragSink?.cancelled(dragged);
    const hover = this.#hover;
    hover?.onMouseLeave?.();
    this.#hover = null;
    this.#lastPointer = null;
    this.#lastDown = null;
    pressed?.onPointerRelease?.();
    if (hover) this.#emit({ type: 'hover', target: null });
  }

  // ---- keyboard inside `WBS_KEY` frames (WP-UI-06; ref/22 §5.3; `DefWindowProc` WM_KEYDOWN/WM_KEYUP/WM_CHAR) ------

  /**
   * A key press for the focused frame (43's router calls it for the keys that no focused edit took):
   *  * `WBS_KEY` frame: Enter presses the child marked `setDefault(true)` as if Space were pressed on it (and focuses
   *    it), Space presses the focused child (`onKeyPress`; the click happens on `keyUp`), Tab / Shift+Tab move the
   *    focus to the next / previous child with `setTabStop(true)`;
   *  * every frame: `onKeyDown(code)` of the focused leaf and, when different, of the focused frame.
   *
   * @returns `true` when the key was a `WBS_KEY` key (Enter, Space, Tab) of a `WBS_KEY` frame: the caller must not
   *   give it to the game or let the browser act on it
   */
  keyDown(code: string, info: KeyInfo): boolean {
    const frame = this.#focusFrame;
    if (frame === this.root) return false;
    const leaf = this.#curFocus;
    let handled = false;
    if (frame.isWndStyle(WBS_KEY)) {
      const enter = code === 'Enter' || code === 'NumpadEnter';
      // Space belongs to the text of an edit (a chat line, a password): it only presses buttons
      if (enter || (code === 'Space' && !isTextLeaf(leaf))) {
        handled = true;
        let target: WndBase = leaf;
        if (enter) {
          const def = frame.childWnds.find((c) => c.isDefault);
          if (def) {
            target = def;
            if (def !== leaf) def.setFocus();
          }
        }
        const press = code === 'Space' || (enter && target.isDefault);
        if (
          press &&
          target.isWndStyle(WBS_CHILD) &&
          this.#keyButton !== target &&
          this.#pressed !== target
        ) {
          this.#keyButton = target;
          target.onKeyPress?.();
        }
      } else if (code === 'Tab') {
        handled = true;
        this.#focusTabStop(frame, info.shift);
      }
    }
    leaf.onKeyDown?.(code, info);
    if (frame !== leaf) frame.onKeyDown?.(code, info);
    return handled;
  }

  /** The key release matching {@link keyDown}: Space (or Enter on the default child) releases the held child. */
  keyUp(code: string, info: KeyInfo): boolean {
    const frame = this.#focusFrame;
    if (frame === this.root) return false;
    const leaf = this.#curFocus;
    let handled = false;
    if (frame.isWndStyle(WBS_KEY)) {
      const enter = code === 'Enter' || code === 'NumpadEnter';
      if (enter || (code === 'Space' && !isTextLeaf(leaf))) {
        handled = true;
        const held = this.#keyButton;
        if (held) {
          this.#keyButton = null;
          if (held.isWindowEnabled()) held.onKeyRelease?.();
        }
      } else if (code === 'Tab') {
        handled = true;
      }
    }
    leaf.onKeyUp?.(code, info);
    if (frame !== leaf) frame.onKeyUp?.(code, info);
    return handled;
  }

  /**
   * The Tab cycle (`WM_CHAR` VK_TAB): from the focused child of the frame forward (backward with Shift) over the
   * frame's children in creation order, wrapping, to the first one with `tabStop`; the focused child itself comes
   * last. Nothing happens without a focused child.
   */
  #focusTabStop(frame: WndBase, backward: boolean): void {
    const kids = frame.childWnds;
    const n = kids.length;
    const cur = frame.focusChild ? kids.indexOf(frame.focusChild) : -1;
    if (cur < 0 || n < 2) return;
    for (let k = 1; k <= n; k += 1) {
      const i = (((backward ? cur - k : cur + k) % n) + n) % n;
      const c = kids[i];
      if (c?.tabStop === true && c.isVisible() && c.isWindowEnabled()) {
        c.setFocus();
        return;
      }
    }
  }

  /** The window under the pointer after the last move (tooltips, cursors). */
  get hover(): WndBase | null {
    return this.#hover;
  }

  /** A mouse button is held on a window or a move / resize runs (`m_bLButtonDown`): cursors stay as they are. */
  get pointerPressed(): boolean {
    return this.#pressed !== null || this.#drag !== null;
  }

  /** The last pointer position in virtual pixels (`null` before the first event): `GetMousePoint()` of the original. */
  get lastPointer(): Point | null {
    return this.#lastPointer;
  }

  // ---- alignment, hide-all (Plan/42 §5.10) -------------------------------------------------------------------

  /**
   * `CWndMgr::AlighWindow(rcOld, rcNew)` (WndManager.cpp:655-686): every frame except the `WBS_MANAGER` ones
   * (root, taskbar, world) is pushed back inside the new layout rect, and a frame that was flush with an old edge
   * stays flush with the new one.
   */
  alignAll(rcOld: Rect, rcNew: Rect): void {
    for (const f of this.#order) {
      if (f.isWndStyle(WBS_MANAGER)) continue;
      const rc = f.getWindowRect(true);
      const p = { x: rc.l, y: rc.t };
      const w = rectWidth(rc);
      const h = rectHeight(rc);
      if (rc.l < rcNew.l) p.x = rcNew.l;
      if (rc.t < rcNew.t) p.y = rcNew.t;
      if (rc.r > rcNew.r) p.x = rcNew.r - w;
      if (rc.b > rcNew.b) p.y = rcNew.b - h;
      if (rc.l === rcOld.l) p.x = rcNew.l;
      if (rc.t === rcOld.t) p.y = rcNew.t;
      if (rc.r === rcOld.r) p.x = rcNew.r - w;
      if (rc.b === rcOld.b) p.y = rcNew.b - h;
      f.move(p);
    }
  }

  /**
   * `hideAll` (numpad `*`): hides every applet frame (not the manager-layer ones) and remembers which were visible;
   * `restoreAll` shows exactly those again. KEEP: a second `hideAll` before `restoreAll` does nothing.
   */
  hideAll(): void {
    if (this.#clearFlag) return;
    this.#clearFlag = true;
    this.#hidden.length = 0;
    for (const w of this.#instances.values()) {
      if (w.destroyed || w.isWndStyle(WBS_MANAGER) || !w.isVisible()) continue;
      this.#hidden.push(w);
      w.setVisible(false);
    }
    this.root.setFocus();
    this.#bump();
  }

  restoreAll(): void {
    if (!this.#clearFlag) return;
    this.#clearFlag = false;
    for (const w of this.#hidden) {
      if (!w.destroyed) w.setVisible(true);
    }
    this.#hidden.length = 0;
    this.#bump();
  }

  get allHidden(): boolean {
    return this.#clearFlag;
  }

  /** `WindowManagerApi.clearAllWindows` (Plan/42 §5.6): the name 43 and 45 use for {@link hideAll}. */
  clearAllWindows(): void {
    this.hideAll();
  }

  /** `WindowManagerApi.restoreWindows`: the name 43 and 45 use for {@link restoreAll}. */
  restoreWindows(): void {
    this.restoreAll();
  }

  /**
   * `WindowManagerApi.cancelDragUnlessFrom(app)`: drops the dragged shortcut unless it was picked up in the window
   * of `app` (the drag controller of WP-UI-12 calls it when a window opens or closes under a drag).
   */
  cancelDragUnlessFrom(app: number): void {
    const from = this.#shortcut?.from?.getFrameWnd();
    if (this.#shortcut && from?.appId !== app) this.drag.cancel();
  }

  // ---- the WindowManagerApi subset that is pure window state (Plan/42 §5.6-§5.7) ----------------------------

  /** The APP id of the focused frame, `null` for the root. */
  focusedApp(): number | null {
    return this.#focusFrame === this.root ? null : (this.#focusFrame as WndNeuz).appId;
  }

  anyOpen(apps: readonly number[]): boolean {
    return apps.some((a) => this.isOpen(a));
  }

  /** `APP_VENDOREX_SELL` or `APP_VENDOREX_BUY` is open (digit keys and Esc are skipped then). */
  isVendorWindowOpen(): boolean {
    return this.isOpen(APP_VENDOREX_SELL) || this.isOpen(APP_VENDOREX_BUY);
  }

  /** Does a UI `<input>`/`<textarea>` have DOM focus? */
  isEditFocused(): boolean {
    return this.editFocusProbe();
  }

  /** The logout countdown window (`m_pLogOutWaitting`, `APP_MESSAGE_TEXT`) exists: game keys are swallowed (§5.6). */
  isLogoutPending(): boolean {
    return this.isOpen(APP_MESSAGE_TEXT);
  }

  /** `APP_STATE_CONFIRM` is open (its Enter/Esc keys act even with another modal open). */
  isStateConfirmOpen(): boolean {
    return this.isOpen(APP_STATE_CONFIRM);
  }

  /**
   * `CWndBase::GetWndBase()`: the top-most closable frame for Esc. Scans the order top -> bottom for a visible frame
   * without `WBS_MANAGER`/`WBS_TOPMOST`, skipping the always-open HUD windows (`ESC_SKIP_APPS`). The skill tree
   * blocks the chain while `APP_QUEITMWARNING` is open (result `null`) and, with unspent point edits, asks its
   * port to open the re-skill warning and returns `null`.
   */
  topClosableWindow(): WndNeuz | null {
    for (let i = this.#order.length - 1; i >= 0; i -= 1) {
      const w = this.#order[i];
      if (!w || !w.isVisible() || w.isWndStyle(WBS_MANAGER | WBS_TOPMOST)) continue;
      if (w.appId === APP_SKILL3) {
        if (this.isOpen(APP_QUEITMWARNING)) return null;
        if (this.skillTreeEsc?.hasUnspentEdits()) {
          this.skillTreeEsc.openWarning();
          return null;
        }
      }
      if (ESC_SKIP_APPS.includes(w.appId)) continue;
      return w;
    }
    return null;
  }

  /**
   * The Esc close step (43 calls it with `topClosableWindow()`): `kept` for the mini-game windows and the guild
   * house menu (found but never destroyed), `closed` after `destroy()`, `none` without a window (the caller then
   * opens the start menu above the taskbar).
   */
  closeTopWindow(w: WndNeuz | null = this.topClosableWindow()): 'none' | 'kept' | 'closed' {
    if (!w) return 'none';
    if (ESC_KEEP_APPS.includes(w.appId)) return 'kept';
    this.destroy(w);
    return 'closed';
  }

  /**
   * Enter while a modal is open and no edit has the focus (Neuz.cpp:1304-1323): the chat window temporarily gets
   * `WBS_MODAL` and the focus; the previous modal frame is remembered. Returns `false` without a chat window.
   */
  focusChatOverModal(): boolean {
    const chat = this.get(APP_COMMUNICATION_CHAT);
    if (!chat) return false;
    if (!this.#tempModal) {
      this.#tempModal = { frame: chat, previous: this.#focusFrame };
    }
    chat.addWndStyle(WBS_MODAL);
    chat.setFocus();
    return true;
  }

  /** After the chat line was sent (or abandoned): drops the temporary modal flag and refocuses the old modal. */
  endChatOverModal(): void {
    const t = this.#tempModal;
    if (!t) return;
    this.#tempModal = null;
    t.frame.delWndStyle(WBS_MODAL);
    if (!t.previous.destroyed && t.previous !== this.root) t.previous.setFocus();
    else this.root.setFocus();
    this.#bump();
  }

  // ---- window rect registry (Plan/42 §21.2) ------------------------------------------------------------------

  /** One row per frame, bottom -> top: where every window is (world input, probes, tests). */
  rects(): WindowRectInfo[] {
    return this.#order.map((f, i) => {
      return {
        app: f.appId,
        key: this.#keys.get(f) ?? '',
        rect: rectCopy(f.rectWindow),
        z: i * 10,
        visible: f.isVisible(),
        focused: this.#focusFrame === f,
      };
    });
  }

  /** `true` when a visible frame covers the point (the world must not receive the pointer there). */
  isOverUi(pt: Point): boolean {
    return this.hitTest(pt).frame !== null || this.modalOpen();
  }
}

/** The focused leaf types text (an `EditCtrl`: it keeps Space for itself). */
function isTextLeaf(w: WndBase): boolean {
  return 'consumesKey' in w;
}

/** A frame and all of its descendants are gone (async work that still holds them checks the flag). */
function markDestroyed(w: WndBase): void {
  w.destroyed = true;
  for (const c of w.childWnds) markDestroyed(c);
}

function defaultEditFocusProbe(): boolean {
  if (typeof document === 'undefined') return false;
  const a = document.activeElement;
  if (!a) return false;
  const tag = a.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || (a as HTMLElement).isContentEditable;
}
