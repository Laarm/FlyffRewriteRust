// The taskbar service (`taskbar/` of Plan/42 §2.1, WP-UI-14): owns the model, creates the `TaskbarWnd` of the layout that
// fits the virtual width, follows viewport changes, hosts the start menu and implements the `TaskbarApi` operations
// that 43 calls (§11.3: page keys, F-key execution).

import { SC } from '../dnd/shortcut';
import { rectXYWH } from '../layout/geometry';
import type { UiImageCache } from '../skin/images';
import type { TooltipService } from '../tooltip/TooltipService';
import type { TaskbarApi } from '../wm/api';
import type { AppletRegistry } from '../wm/applets';
import type { WindowManager } from '../wm/WindowManager';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import { chooseTaskbarLayout, TASKBAR_H } from './layout';
import { TaskbarModel } from './model';
import { ActionQueue } from './queue';
import { StartMenu } from './StartMenu';
import { TaskbarWnd, type TaskbarPorts } from './TaskbarWnd';

export interface TaskbarServiceDeps {
  readonly wm: WindowManager;
  readonly applets: AppletRegistry;
  readonly images: UiImageCache;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  readonly features: () => ClientFeatureConfig | undefined;
  readonly isGm: () => boolean;
  /** 45's ports; the default does nothing (a context without features executes only applets). */
  readonly ports?: TaskbarPorts;
  /** Reports the real taskbar height to the viewport owner (`UiScaler.setTaskbarHeight`). */
  readonly setInset?: (height: number) => void;
}

/** The ports of a UI without feature modules: nothing is executed, nothing is sent, no icons beyond the applets'. */
export const NULL_TASKBAR_PORTS: TaskbarPorts = {
  execute: () => undefined,
  slotIcon: () => null,
  actions: {
    addApplet: () => undefined,
    removeApplet: () => undefined,
    addItem: () => undefined,
    removeItem: () => undefined,
    sendQueue: () => undefined,
  },
};

export class Taskbar implements TaskbarApi {
  readonly model = new TaskbarModel();
  readonly startMenu: StartMenu;
  readonly #d: TaskbarServiceDeps;
  #ports: TaskbarPorts;
  /** The editing mechanics of the queue row (WP-UI-42); used when the ports bring `queueActions` and no `queue` port of their own. */
  readonly queue = new ActionQueue({
    model: this.model,
    actions: () => this.#ports.queueActions,
    skill: (id) => this.#ports.skills?.info(0, id) ?? null,
    say: (tid, ...args) => {
      this.#ports.messageFmt?.(tid, ...args);
    },
  });
  #wnd: TaskbarWnd | null = null;
  #off: (() => void) | null = null;

  constructor(deps: TaskbarServiceDeps) {
    this.#d = deps;
    this.#ports = deps.ports ?? NULL_TASKBAR_PORTS;
    this.startMenu = new StartMenu({
      wm: deps.wm,
      applets: deps.applets,
      images: deps.images,
      getText: deps.getText,
      features: deps.features,
      isGm: deps.isGm,
      owner: () => this.#wnd,
      executeApplet: (app) => {
        deps.wm.objectExecutorApplet(app);
      },
    });
  }

  /** Installs 45's ports (the feature layer calls this at boot). */
  setPorts(ports: TaskbarPorts): void {
    this.#ports = ports;
  }

  /** The live taskbar window. */
  get window(): TaskbarWnd | null {
    return this.#wnd && !this.#wnd.destroyed ? this.#wnd : null;
  }

  /** Creates the taskbar (idempotent) and starts following the viewport. Returns the window. */
  create(): TaskbarWnd | null {
    if (this.window) return this.#wnd;
    const wm = this.#d.wm;
    const spec = chooseTaskbarLayout(wm.viewport.r - wm.viewport.l);
    const wnd = new TaskbarWnd({
      spec,
      model: this.model,
      ports: this.#portsProxy,
      applets: this.#d.applets,
      images: this.#d.images,
      tooltip: this.#d.tooltip,
      getText: this.#d.getText,
      viewport: () => ({
        width: wm.viewport.r - wm.viewport.l,
        layoutBottom: wm.viewport.b - TASKBAR_H,
      }),
      executeApplet: (app) => {
        wm.objectExecutorApplet(app);
      },
      openStartMenu: () => {
        wm.openStartMenu(true);
      },
    });
    const opened = wm.open(spec.app, () => wnd);
    if (!opened) return null;
    this.#wnd = wnd;
    this.#d.setInset?.(TASKBAR_H);
    wm.appletHooks = { ...wm.appletHooks, openStartMenu: () => void this.startMenu.open() };
    this.#off?.();
    this.#off = wm.onEvent((e) => {
      if (e.type === 'viewport') this.relayout();
      else if (e.type === 'pointerDown' && this.startMenu.isOpen()) {
        // a click on the taskbar closes the menu (WndTaskBar.cpp: `m_wndMenu` hides when the taskbar is clicked)
      }
    });
    return wnd;
  }

  /** The ports through which the window reaches the CURRENT ports object (`setPorts` after creation works). */
  readonly #portsProxy: TaskbarPorts = (() => {
    const cur = (): TaskbarPorts => this.#ports;
    const queue = this.queue;
    return {
      execute: (sc) => {
        cur().execute(sc);
      },
      slotIcon: (sc) => cur().slotIcon(sc),
      slotTooltip: (sc) => cur().slotTooltip?.(sc) ?? null,
      slotTooltipRich: (sc, rect) => cur().slotTooltipRich?.(sc, rect) ?? false,
      newFontColor: () =>
        cur().newFontColor?.() ?? this.#d.features()?.features.new_font_color ?? true,
      slotIsEquip: (sc) => cur().slotIsEquip?.(sc) ?? false,
      message: (tid) => {
        cur().message?.(tid);
      },
      iconOf: (sc) => cur().iconOf?.(sc) ?? null,
      // optional members the window tests for presence: forwarded from the current ports
      get tradeRequest() {
        return cur().tradeRequest;
      },
      get skills() {
        return cur().skills;
      },
      get items() {
        return cur().items;
      },
      get queue() {
        const own = cur().queue;
        if (own) return own;
        return cur().queueActions ? queue : undefined;
      },
      get queueActions() {
        return cur().queueActions;
      },
      messageFmt: (tid: number, ...args: string[]) => {
        cur().messageFmt?.(tid, ...args);
      },
      actionPointsFree: () => cur().actionPointsFree?.() ?? false,
      animating: () => cur().animating?.() ?? false,
      queueTooltip: (ap: number, slot: number, cost: number) =>
        cur().queueTooltip?.(ap, slot, cost) ?? null,
      actions: {
        addApplet: (s, sc) => {
          cur().actions.addApplet(s, sc);
        },
        removeApplet: (s) => {
          cur().actions.removeApplet(s);
        },
        addItem: (p, s, sc) => {
          cur().actions.addItem(p, s, sc);
        },
        removeItem: (p, s) => {
          cur().actions.removeItem(p, s);
        },
        sendQueue: (q) => {
          cur().actions.sendQueue(q);
        },
      },
    };
  })();

  /** The viewport changed: another layout may fit now (recreate), else the frame just follows. */
  relayout(): void {
    const w = this.window;
    if (!w) return;
    const wm = this.#d.wm;
    const spec = chooseTaskbarLayout(wm.viewport.r - wm.viewport.l);
    if (spec.app === w.spec.app) {
      w.replace();
      return;
    }
    this.destroy(false);
    this.create();
  }

  /** Removes the taskbar (leaving the world). */
  destroy(releaseInset = true): void {
    const w = this.window;
    this.startMenu.dispose();
    this.#off?.();
    this.#off = null;
    if (w) this.#d.wm.destroy(w);
    this.#wnd = null;
    if (releaseInset) this.#d.setInset?.(0);
  }

  // ---- TaskbarApi (43 §3) -------------------------------------------------------------------------------------

  /** `F1`-`F9`: executes item slot `i` of the current page (§11.5). */
  executeItemSlot(i: number): void {
    const sc = this.model.itemAt(i);
    if (!sc || sc.kind === SC.NONE) return;
    this.window?.executeShortcut(sc);
    if (!this.window) {
      if (sc.kind === SC.APPLET) this.#d.wm.objectExecutorApplet(sc.id);
      else this.#ports.execute(sc);
    }
  }

  /** `Digit1`-`Digit8`: page 0..7. */
  setItemPage(n: number): void {
    this.model.setPage(n);
  }

  page(): number {
    return this.model.get().page;
  }

  /** The equipment latch of 43 §3.4: the item slot holds an ITEM shortcut whose item is worn equipment. */
  slotIsEquipItem(i: number): boolean {
    const sc = this.model.itemAt(i);
    return sc !== null && sc.kind === SC.ITEM && this.#ports.slotIsEquip?.(sc) === true;
  }
}

/** The taskbar frame rect for a viewport (tests): full width at the bottom. */
export function taskbarRect(width: number, height: number) {
  return rectXYWH(0, height - TASKBAR_H, width, TASKBAR_H);
}
