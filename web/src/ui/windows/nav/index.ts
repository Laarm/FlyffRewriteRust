// `NavWindowsService` (`ui.nav`, Plan/42 §19.4 C / D / E / L / S; WP-UI-36): the navigator, the TELE16 teleporter, the inspection window, the
// collecting panel, the exchange list and the world-map overlay.
//
//   ui.nav.setPorts({ navigator, tele16, worldMap, collecting })    45 WP-CFEAT-28, -29, -35: the view and the actions of the applets
//   (applets)   APP_NAVIGATOR (hotkey N), APP_TELE16 (hotkey V), APP_COLLECTING open through the applet registry; APP_MAP (hotkey M) is the
//               world-map overlay, toggled through `appletHooks.toggleWorldMap`
//   ui.nav.openQueryEquip(ports)       APP_QUERYEQUIP: another player's equipment (a new inspection replaces the open window)
//   ui.nav.worldMap                    the view source the overlay layer (`WorldMapLayer`) renders
//
// The factories read the ports when a window is created, so ports installed later reach windows opened later. `APP_RESTATE_CONFIRM` (the
// re-stat scroll) is the `ui.itemUse` window of WP-UI-28 and `APP_DIALOG_EVENT` (the exchange list) the `windows/exchange` shell of the
// exchange task (WP-CFEAT-26); neither is repeated here.

import {
  APP_COLLECTING,
  APP_NAVIGATOR,
  APP_QUERYEQUIP,
  APP_TELE16,
} from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import type { WndNeuz } from '../../wnd/WndNeuz';
import {
  constantView,
  EMPTY_COLLECTING_VIEW,
  EMPTY_NAVIGATOR_VIEW,
  type CollectingPorts,
  type NavigatorPorts,
  type QueryEquipPorts,
  type Tele16Ports,
  type ViewSource,
  type WorldMapPorts,
  type WorldMapView,
} from './ports';
import { WndCollecting } from './WndCollecting';
import { WndNavigator } from './WndNavigator';
import { WndQueryEquip } from './WndQueryEquip';
import { WndTele16 } from './WndTele16';

export * from './ports';
export { WndNavigator, NAV_MIN, NAV_MAX } from './WndNavigator';
export { WndTele16, TELE16_BOSS_WIDTH } from './WndTele16';
export { WndQueryEquip } from './WndQueryEquip';
export { WndCollecting, batteryText, levelText } from './WndCollecting';
export { NavMapCtrl } from './NavMapCtrl';
export { WorldMapLayer } from './WorldMapLayer';

const ELEMENT_ATLAS = 'icon/icon_monelemantkind.inc';
const ELEMENT_TEXTURE = 'icon/icon_monelemantkind.dds';

export interface NavWindowDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

/** The ports before 45 installs the real ones. */
export interface NavPortSet {
  navigator: NavigatorPorts;
  tele16: Tele16Ports;
  worldMap: WorldMapPorts;
  collecting: CollectingPorts;
}

const CLOSED_MAP: WorldMapView = {
  open: false,
  box: { l: 0, t: 0, r: 0, b: 0 },
  rate: 1,
  mapNo: 0,
  texture: null,
  buttons: [],
  arrows: [],
  icons: [],
  teleporter: false,
  hoverIcon: null,
  viewH: 0,
};

export const NULL_NAV_PORTS: NavPortSet = {
  navigator: {
    view: constantView(EMPTY_NAVIGATOR_VIEW),
    actions: {
      zoom: () => undefined,
      toggleFilter: () => undefined,
      lButtonDown: () => undefined,
      lButtonUp: () => undefined,
      rButtonDown: () => [],
      doubleClick: () => undefined,
      hover: () => null,
      pickNpc: () => undefined,
      restore: () => undefined,
    },
    paint: () => undefined,
  },
  tele16: {
    view: constantView({ entries: [] }),
    actions: { teleport: () => undefined },
  },
  worldMap: { view: constantView(CLOSED_MAP), hover: () => undefined, toggle: () => undefined },
  collecting: {
    view: constantView(EMPTY_COLLECTING_VIEW),
    actions: { toggle: () => undefined },
  },
};

export class NavWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #atlases: UiAtlases;
  readonly #getText: (tid: number) => string;
  #ports: NavPortSet = NULL_NAV_PORTS;
  readonly #mapListeners = new Set<() => void>();
  #mapOff: (() => void) | null = null;

  /** The view source of the world-map overlay: stable while the ports change (the layer subscribes once). */
  readonly worldMap: ViewSource<WorldMapView> = {
    get: () => this.#ports.worldMap.view.get(),
    subscribe: (fn) => {
      this.#mapListeners.add(fn);
      this.#bindMap();
      return () => {
        this.#mapListeners.delete(fn);
        if (this.#mapListeners.size === 0) {
          this.#mapOff?.();
          this.#mapOff = null;
        }
      };
    },
  };

  constructor(deps: NavWindowDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#atlases = deps.atlases;
    this.#getText = deps.getText;
    const applets = deps.wm.applets;
    applets.register({ app: APP_NAVIGATOR, factory: () => this.createNavigator() });
    applets.register({ app: APP_TELE16, factory: () => this.createTele16() });
    applets.register({ app: APP_COLLECTING, factory: () => this.createCollecting() });
    // `APP_MAP` is a toggle of the overlay, not a window (`CWndMap::Initialize` returns FALSE)
    deps.wm.appletHooks = {
      ...deps.wm.appletHooks,
      toggleWorldMap: () => {
        this.#ports.worldMap.toggle();
      },
    };
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<NavPortSet>): void {
    this.#ports = { ...this.#ports, ...ports };
    if (ports.worldMap) {
      this.#mapOff?.();
      this.#mapOff = null;
      if (this.#mapListeners.size > 0) this.#bindMap();
      for (const fn of [...this.#mapListeners]) fn();
    }
  }

  get ports(): NavPortSet {
    return this.#ports;
  }

  #bindMap(): void {
    if (this.#mapOff) return;
    this.#mapOff = this.#ports.worldMap.view.subscribe(() => {
      for (const fn of [...this.#mapListeners]) fn();
    });
  }

  // ---- the applets ---------------------------------------------------------------------------------------------------------

  createNavigator(): WndNavigator {
    return new WndNavigator({
      ports: this.#ports.navigator,
      getText: this.#getText,
      tooltip: this.#tooltip,
    });
  }

  createTele16(): WndTele16 {
    return new WndTele16({
      ports: this.#ports.tele16,
      tooltip: this.#tooltip,
      elementPath: ELEMENT_TEXTURE,
      elementCut: (i) => {
        const s = this.#atlases.info(ELEMENT_ATLAS)?.sprites[i];
        return s ? { x: s.x, y: s.y } : { x: i * 16, y: 0 };
      },
    });
  }

  createCollecting(): WndCollecting {
    return new WndCollecting({
      ports: this.#ports.collecting,
      tooltip: this.#tooltip,
      getText: this.#getText,
    });
  }

  navigator(): WndNavigator | null {
    const w = this.#wm.get(APP_NAVIGATOR);
    return w instanceof WndNavigator ? w : null;
  }
  tele16(): WndTele16 | null {
    const w = this.#wm.get(APP_TELE16);
    return w instanceof WndTele16 ? w : null;
  }
  collecting(): WndCollecting | null {
    const w = this.#wm.get(APP_COLLECTING);
    return w instanceof WndCollecting ? w : null;
  }

  // ---- the windows the server opens ------------------------------------------------------------------------------------------

  /** `OnQueryEquip`: the equipment of another player; an open inspection is replaced. */
  openQueryEquip(ports: QueryEquipPorts): WndQueryEquip | null {
    return this.#replace(
      APP_QUERYEQUIP,
      () => new WndQueryEquip({ ports, tooltip: this.#tooltip }),
    );
  }

  queryEquip(): WndQueryEquip | null {
    const w = this.#wm.get(APP_QUERYEQUIP);
    return w instanceof WndQueryEquip ? w : null;
  }

  #replace<T extends WndNeuz>(app: number, build: () => T): T | null {
    const old = this.#wm.get(app);
    if (old) {
      old.destroy();
      this.#wm.flushDestroyed();
    }
    return this.#wm.open(app, build);
  }
}
