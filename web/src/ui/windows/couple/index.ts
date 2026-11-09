// `CoupleWindowsService` (`ui.couple`, Plan/42 §19.4 K / O; WP-UI-52): the couple window, its two pages, the couple message window and the honor page of the character window.
//
//   ui.couple.setPorts({ couple })                  45 WP-CFEAT-40: the views and the actions of the couple window and its pages
//   (applet)   APP_COUPLE_MAIN (hotkey F, start menu `TID_GAME_COUPLE`) opens through the applet registry
//   ui.couple.openMessage(request)                  the couple message window (`CWndCoupleMessage`, OK / Cancel on its own `APP_MESSAGEBOX`); a new one replaces the open one
//   ui.char.setPorts({ honor })                     the honor page (`HonorPage`) is built by the character window; its ports ride in the character ports
//
// The service registers the applet so that the taskbar, the start menu and the hotkey open it; the factory reads the ports at the moment the window is created, so ports installed later reach
// windows opened later.

import { APP_COUPLE_MAIN, APP_MESSAGEBOX } from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { COUPLE_LAZY } from './lazy';
import {
  constantView,
  COUPLE_MESSAGE_KEY,
  type CouplePorts,
  type CoupleMessageRequest,
  EMPTY_COUPLE_INFO,
  EMPTY_COUPLE_SKILL,
} from './ports';
import { WndCoupleManager } from './WndCoupleManager';
import type { WndCoupleMessage } from './WndCoupleMessage';

export * from './ports';
export { WndCoupleManager } from './WndCoupleManager';
export { CoupleInfoPage } from './CoupleInfoPage';
export { CoupleSkillPage } from './CoupleSkillPage';
export { HonorPage, honorDeps } from './HonorPage';
export type { WndCoupleMessage } from './WndCoupleMessage';

export interface CoupleWindowDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

/** The ports before 45 installs the real ones: a single player, actions that do nothing. */
export const NULL_COUPLE_PORTS: CouplePorts = {
  info: constantView(EMPTY_COUPLE_INFO),
  skill: constantView(EMPTY_COUPLE_SKILL),
  actions: { breakUp: () => undefined, skillPageOpened: () => undefined },
};

export class CoupleWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #atlases: UiAtlases;
  readonly #getText: (tid: number) => string;
  #ports: CouplePorts = NULL_COUPLE_PORTS;

  constructor(deps: CoupleWindowDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#atlases = deps.atlases;
    this.#getText = deps.getText;
    deps.wm.applets.register({ app: APP_COUPLE_MAIN, factory: () => this.createManager() });
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<{ couple: CouplePorts }>): void {
    if (ports.couple) this.#ports = ports.couple;
  }

  get ports(): CouplePorts {
    return this.#ports;
  }

  // ---- the applet ---------------------------------------------------------------------------------------------------------

  createManager(): WndCoupleManager {
    return new WndCoupleManager({
      ports: this.#ports,
      tooltip: this.#tooltip,
      atlases: this.#atlases,
      getText: this.#getText,
    });
  }

  manager(): WndCoupleManager | null {
    const w = this.#wm.get(APP_COUPLE_MAIN);
    return w instanceof WndCoupleManager ? w : null;
  }

  // ---- the couple message window ----------------------------------------------------------------------------------------

  /**
   * `SAFE_DELETE( m_pWndCoupleMessage ); new CWndCoupleMessage; Initialize()`: opens the box, replacing the open one. `null` while the chunk of the dialog is loading (the box then
   * opens by itself).
   */
  openMessage(request: CoupleMessageRequest): WndCoupleMessage | null {
    const lazy = COUPLE_LAZY.WndCoupleMessage;
    const open = (C: NonNullable<typeof lazy.ctor>): WndCoupleMessage | null => {
      const old = this.#wm.get(APP_MESSAGEBOX, COUPLE_MESSAGE_KEY);
      if (old) {
        this.#wm.destroy(old);
        this.#wm.flushDestroyed();
      }
      return this.#wm.open(APP_MESSAGEBOX, () => new C(request), COUPLE_MESSAGE_KEY);
    };
    const ctor = lazy.ctor;
    if (ctor) return open(ctor);
    lazy.load().then(
      (c) => {
        open(c);
      },
      () => undefined,
    );
    return null;
  }

  /** The open couple message window. */
  message(): WndCoupleMessage | null {
    const w = this.#wm.get(APP_MESSAGEBOX, COUPLE_MESSAGE_KEY);
    const ctor = COUPLE_LAZY.WndCoupleMessage.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /** Closes the couple message window without an answer. */
  closeMessage(): void {
    this.message()?.destroy();
  }
}
