// The `__flyffDebug.ui` section of the debug probe (Plan/42 §21.2): what the Playwright specs and the manual QA read
// to see the window state without poking at the DOM. `createUi` builds it (`ui.probe`); the boot code publishes it with
// `ctx.probe.extend('ui', ui.probe)`, the gallery does the same in its own page.
//
//   order()     one row per frame, bottom to top: app, instance key, rect, z, visible, focused
//   open/close  open (a generic layout window unless the host supplies an opener) or close an app by id or APP_ name
//   tooltip()   the text of the tooltip that is showing, `null` when none
//   drag()      the shortcut being dragged
//   escSteps()  the Esc steps 43's router took (the router appends to the list it handed in)
//   actions     the recorded port calls of the fixtures' mock ports (WP-UI-10)
//   catalog()   the window catalog

import type { WINDOW_CATALOG } from './catalog';
import type { Shortcut } from './dnd/shortcut';
import type { LayoutRegistry } from './layout/registry';
import type { TooltipService } from './tooltip/TooltipService';
import type { WindowManager, WindowRectInfo } from './wm/WindowManager';
import { LayoutWindow } from './wnd/LayoutWindow';

/** One recorded call of a mock port. */
export interface ProbeAction {
  port: string;
  method: string;
  args: unknown[];
}

export interface UiProbe {
  order(): WindowRectInfo[];
  open(app: number | string, fixture?: string): void;
  close(app: number | string): void;
  tooltip(): string | null;
  /** The budget counters of the item tooltip host: provider calls, compositions, cache hits (WP-UI-20). */
  tooltipStats(): { numberCalls: number; compositions: number; cacheHits: number };
  drag(): Shortcut | null;
  escSteps(): string[];
  actions: ProbeAction[];
  catalog(): typeof WINDOW_CATALOG;
}

/** What the probe reads from the UI (`UiApi` satisfies it). */
export interface UiProbeSource {
  readonly wm: WindowManager;
  readonly registry: LayoutRegistry;
  readonly tooltip: TooltipService;
  readonly catalog: typeof WINDOW_CATALOG;
}

export interface UiProbeOptions {
  /** Opens an app with a fixture; default: a bare `LayoutWindow` of the layout (any APP id, no behaviour). */
  open?: (app: number, fixture: string | undefined) => void;
  /** The shared action log of the mock ports (default: a fresh list). */
  actions?: ProbeAction[];
  /** The Esc steps taken so far (43's router owns the list). */
  escSteps?: () => readonly string[];
}

/** The numeric id of an app given as an id or an `APP_NAME` (`undefined` for an unknown name). */
export function resolveApp(registry: LayoutRegistry, app: number | string): number | undefined {
  if (typeof app === 'number') return app;
  return registry.resolve(app as `APP_${string}`)?.id;
}

export function createUiProbe(ui: UiProbeSource, o: UiProbeOptions = {}): UiProbe {
  const openDefault = (app: number): void => {
    ui.wm.open(app, () => new LayoutWindow(app))?.moveParentCenter();
  };
  return {
    order: () => ui.wm.rects(),
    open: (app, fixture) => {
      const id = resolveApp(ui.registry, app);
      if (id !== undefined) (o.open ?? openDefault)(id, fixture);
    },
    close: (app) => {
      const id = resolveApp(ui.registry, app);
      if (id !== undefined) ui.wm.close(id);
    },
    tooltip: () => ui.tooltip.text(),
    tooltipStats: () => ({ ...ui.tooltip.stats }),
    drag: () => ui.wm.draggedShortcut,
    escSteps: () => [...(o.escSteps?.() ?? [])],
    actions: o.actions ?? [],
    catalog: () => ui.catalog,
  };
}
