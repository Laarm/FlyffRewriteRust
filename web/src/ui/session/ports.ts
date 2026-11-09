// The ports of the session shell windows (Plan/42 §14.2, implemented by 45 WP-CFEAT-08; `ui/` never imports `features/`).

/** What the Logout and Quit boxes ask for when their OK button is pressed (`CWndLogOut` / `CWndQuit`). */
export interface SessionActions {
  /** Leave the world to the character select. The leave rule (safe region, game master, countdown) is 45's. */
  logout(): void;
  /** Leave the session. */
  quit(): void;
  /**
   * The Logout applet is refused before any box opens (`IsAttackMode`: 45 writes `TID_PK_COMBAT_END` into the chat);
   * answer `false` to refuse. Quit has no such check.
   */
  mayLogout?(): boolean;
}

/** What the countdown window (`CWndLogOutWaitting`) shows and does. */
export interface LogoutCountdownView {
  /** The whole seconds left; the window shows `text(n)` while `n > 0` and ends the countdown at 0. */
  secondsLeft(): number;
  /** `TID_PK_TIME_END` formatted with the seconds ("Quit after N sec."). */
  text(seconds: number): string;
  /** The countdown ran out: perform the leave (`WM_LOGOUT` / `WM_CLOSE`). The window closes itself afterwards. */
  expired?(): void;
  /** The window was closed without running out (the world was left, a test closed it). */
  closed?(): void;
}

export interface SessionPorts {
  actions: SessionActions;
}

/** Before 45 installs the real ports: nothing happens. */
export const NULL_SESSION_ACTIONS: SessionActions = {
  logout: () => undefined,
  quit: () => undefined,
};
