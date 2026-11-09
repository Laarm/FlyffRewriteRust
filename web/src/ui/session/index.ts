// `SessionService` (`ui.session`, Plan/42 §12.4, §14.2; WP-UI-17): the Logout / Quit boxes, the logout countdown window
// and the "logout pending" state the key router reads.
//
//   ui.session.setActions({logout, quit, mayLogout?})   45 WP-CFEAT-08: what OK of the two boxes does
//   ui.session.openCountdown(view, isLogOut?)           the countdown window on APP_MESSAGE_TEXT
//   ui.session.countdownOpen                            = wm.isLogoutPending()
//
// The service installs the `openLogout` / `openQuit` hooks of the applet registry (the start menu entries and the
// taskbar slots reach the boxes through `wm.objectExecutorApplet`) and registers the two applets so that they exist.

import { APP_LOGOUT, APP_MESSAGE_TEXT, APP_QUIT } from '../../gen/defs/resdata';
import type { MessageBoxApi } from '../msgbox/MessageBoxService';
import type { WindowManager } from '../wm/WindowManager';
import { LogoutCountdownWnd } from './LogoutCountdownWnd';
import { NULL_SESSION_ACTIONS, type LogoutCountdownView, type SessionActions } from './ports';
import { SessionBox } from './SessionBoxes';

export { LogoutCountdownWnd } from './LogoutCountdownWnd';
export { NULL_SESSION_ACTIONS } from './ports';
export type { LogoutCountdownView, SessionActions, SessionPorts } from './ports';
export { SessionBox } from './SessionBoxes';
export type { SessionBoxKind } from './SessionBoxes';

export interface SessionServiceDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  /** `getText(tid)` of the texts bundle (`''` while it is missing). */
  readonly getText: (tid: number) => string;
}

export class SessionService {
  readonly #wm: WindowManager;
  readonly #msgbox: MessageBoxApi;
  readonly #getText: (tid: number) => string;
  #actions: SessionActions = NULL_SESSION_ACTIONS;

  constructor(deps: SessionServiceDeps) {
    this.#wm = deps.wm;
    this.#msgbox = deps.msgbox;
    this.#getText = deps.getText;
    const box = (kind: 'logout' | 'quit') => (): SessionBox =>
      new SessionBox(kind, this.#getText, () => this.#actions);
    deps.wm.applets.register({ app: APP_LOGOUT, factory: box('logout') });
    deps.wm.applets.register({ app: APP_QUIT, factory: box('quit') });
    deps.wm.appletHooks = {
      ...deps.wm.appletHooks,
      openLogout: () => {
        this.openLogout();
      },
      openQuit: () => {
        this.openQuit();
      },
    };
  }

  /** Installs the actions of 45 (`logout`, `quit`, the attack-mode pre-check of Logout). */
  setActions(actions: SessionActions): void {
    this.#actions = actions;
  }

  /** `APP_LOGOUT`: refused by the pre-check (combat), else the confirmation box. Returns the box or `null`. */
  openLogout(): SessionBox | null {
    if (this.#actions.mayLogout?.() === false) return null;
    return this.#open('logout');
  }

  /** `APP_QUIT`: the confirmation box. */
  openQuit(): SessionBox {
    return this.#open('quit');
  }

  #open(kind: 'logout' | 'quit'): SessionBox {
    const box = new SessionBox(kind, this.#getText, () => this.#actions);
    this.#msgbox.openCustom(box);
    return box;
  }

  /**
   * Opens the countdown window (replaces a running one: `m_pLogOutWaitting` is a singleton, a second request while it
   * exists does nothing in the original). Returns the existing window when there is one.
   */
  openCountdown(view: LogoutCountdownView, isLogOut = false): LogoutCountdownWnd | null {
    const existing = this.#wm.get(APP_MESSAGE_TEXT);
    if (existing instanceof LogoutCountdownWnd) return existing;
    const w = this.#wm.open(APP_MESSAGE_TEXT, () => new LogoutCountdownWnd(view, isLogOut));
    return w instanceof LogoutCountdownWnd ? w : null;
  }

  /** The countdown window exists: keys do not reach the game. */
  get countdownOpen(): boolean {
    return this.#wm.isLogoutPending();
  }
}
