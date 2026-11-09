// `PocketWindowsService` (`ui.pockets`, Plan/42 §19.4 E; WP-UI-25): the backpack applet `APP_BAG_EX` (hotkey B, the start menu) and its ports.
//
//   ui.pockets.setPorts({view, actions})    45 (`features/pockets`): the three pockets and what the drops do
//   ui.pockets.open()                       opens (or focuses) the window through the window manager
//
// The service registers the applet (`ui.applets`), so that the taskbar, the start menu and the hotkey open it; the factory reads the ports at the moment a window is
// created, so ports installed later reach windows opened later.

import { APP_BAG_EX } from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { constantView, type PocketPorts, type PocketView } from './ports';
import { WndBagEx } from './WndBagEx';

export * from './ports';
export { WndBagEx } from './WndBagEx';
export {
  BAG_ICON_EMPTY,
  BAG_ICON_NAMES,
  BAG_ICON_USED,
  BAG_NOUSE_NAMES,
  NOUSE_ALPHA,
  NOUSE_TEXTURE,
  POCKET_TAB_NAMES,
} from './names';

export interface PocketWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
}

const EMPTY_VIEW: PocketView = {
  version: 0,
  permanent: false,
  newFontColor: true,
  pockets: [],
};

/** The ports before 45 installs the real ones: no pockets, actions that do nothing. */
export const NULL_POCKET_PORTS: PocketPorts = {
  view: constantView(EMPTY_VIEW),
  actions: {
    moveIn: () => false,
    moveBetween: () => false,
    availBag: () => false,
    closed: () => undefined,
  },
};

export class PocketWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;
  #ports: PocketPorts = NULL_POCKET_PORTS;

  constructor(deps: PocketWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
    deps.wm.applets.register({ app: APP_BAG_EX, factory: () => this.createBag() });
  }

  /** Installs the ports of 45. */
  setPorts(ports: Partial<PocketPorts>): void {
    this.#ports = { ...this.#ports, ...ports };
  }

  get ports(): PocketPorts {
    return this.#ports;
  }

  /** The window factory (also used by the gallery and the tests). */
  createBag(): WndBagEx {
    return new WndBagEx({
      view: this.#ports.view,
      actions: this.#ports.actions,
      tooltip: this.#tooltip,
      getText: this.#getText,
    });
  }

  /** Opens (or focuses) the window; `null` when it refused to exist (a trade, a shop, ... is open). */
  open(): WndBagEx | null {
    const w = this.#wm.open<WndBagEx>(APP_BAG_EX);
    return w && !w.destroyed && !w.refused ? w : null;
  }

  /** The open window, `null` when none. */
  get window(): WndBagEx | null {
    const w = this.#wm.get(APP_BAG_EX);
    return w instanceof WndBagEx ? w : null;
  }
}
