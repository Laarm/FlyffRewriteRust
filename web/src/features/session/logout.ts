// Logout and quit with the countdown (Plan/45 §7.13, WP-CFEAT-08; 12 §12.2-§12.4, §18.5; 42 §14.2 `ui.session`):
//
//  * refused while the own player is in attack mode: the chat line `TID_PK_COMBAT_END` and NOTHING is sent (client-only
//    rule, KEEP, `src:_Interface/WndManager.cpp:2665-2680`; the combat runtime reports attack mode from M3 on, until
//    then the hook is `() => false`). The Logout applet applies the same pre-check before its confirmation box opens
//    (`ui.session.setActions({ mayLogout })`),
//  * a safe region (`region_attr & RA_SAFETY`), authority >= GAMEMASTER or world 202: Logout sends `ReturnToCharSelect` at
//    once (state `Leaving` until `CharList`), Quit closes the WebSocket and drops the token (state `Login`),
//  * elsewhere: the countdown window `CWndLogOutWaitting` on `APP_MESSAGE_TEXT` ("Quit after N sec.", `TID_PK_TIME_END`,
//    `gameplay.timewait_close_s` = 10 s) with `ReqLeave` sent when it starts; at 0 the same two actions run. The window has
//    no close button and while it exists the key router swallows the game keys (Esc included, `wm.isLogoutPending()`):
//    a countdown cannot be cancelled by the player (the server's penalty time runs anyway, 12 §12.3). It closes by itself
//    when the session leaves the world meanwhile (a kick, a lost connection).

import { AUTH_GAMEMASTER } from '../../app/hosts';
import { quitSession, requestLeave, returnToCharSelect } from '../../game/session/actions';
import { APP_MESSAGE_TEXT } from '../../gen/defs/resdata';
import { WI_WORLD_GUILDWAR } from '../../gen/defs/world';
import { TID_PK_COMBAT_END, TID_PK_TIME_END } from '../../gen/defs/text';
import type { LogoutCountdownView } from '../../ui/session/ports';
import type { FeatureContext } from '../common/module';
import { formatTid, sysChat } from '../common/text';

export type LogoutKind = 'logout' | 'quit';

export type LogoutStart =
  /** Refused: attack mode (the chat line was written) or no world. */
  | 'refused'
  /** Done at once (safe region, game master, world 202). */
  | 'immediate'
  /** The countdown window is open. */
  | 'countdown'
  /** A countdown is already running. */
  | 'running';

export interface LogoutController {
  /** `APP_LOGOUT`: leave the world to the character select. */
  readonly logout: () => LogoutStart;
  /** `APP_QUIT`: leave the session (the login window shows). */
  readonly quit: () => LogoutStart;
  /**
   * The pre-check of the Logout applet (`ui.session` `mayLogout`): `false` in attack mode, after the chat line
   * `TID_PK_COMBAT_END`.
   */
  readonly mayLogout: () => boolean;
  /** The own player's attack mode (`CMover::IsAttackMode`; wired by the combat runtime, M3). */
  readonly setAttackModeProbe: (fn: () => boolean) => void;
  /** Closes a running countdown window without leaving (the session ended meanwhile; tests). */
  readonly cancel: () => void;
  /** `true` while the countdown window is open (the key router swallows keys, 42 §5.6). */
  readonly counting: boolean;
  /** The whole seconds left of a running countdown (0 when none). */
  readonly secondsLeft: number;
}

export interface LogoutDeps {
  /** Test seams: the monotonic clock of the countdown and the timer of the fallback without a UI. */
  readonly now?: () => number;
  readonly setTimeout?: (fn: () => void, ms: number) => unknown;
  readonly clearTimeout?: (id: unknown) => void;
}

interface Run {
  readonly kind: LogoutKind;
  readonly deadline: number;
  timer: unknown;
}

export function createLogoutController(
  fc: FeatureContext,
  deps: LogoutDeps = {},
): LogoutController {
  const ctx = fc.app;
  const now = deps.now ?? (() => ctx.now());
  const startTimer = deps.setTimeout ?? ((fn: () => void, ms: number) => setTimeout(fn, ms));
  const stopTimer =
    deps.clearTimeout ??
    ((id: unknown) => {
      clearTimeout(id as number);
    });
  let attackMode: () => boolean = () => false;
  let run: Run | null = null;

  const immediate = (): boolean => {
    const st = ctx.stores.sessionStore.getState();
    const world = ctx.stores.worldStore.getState();
    return (
      world.region.safe || st.authority >= AUTH_GAMEMASTER || world.worldId === WI_WORLD_GUILDWAR
    );
  };

  const finish = (kind: LogoutKind): void => {
    if (ctx.machine.screen.s !== 'InGame') return; // the session ended during the countdown
    if (kind === 'logout') returnToCharSelect(ctx);
    else void quitSession(ctx);
  };

  const secondsLeftOf = (r: Run): number => Math.max(0, Math.ceil((r.deadline - now()) / 1000));

  const mayLogout = (): boolean => {
    if (!attackMode()) return true;
    sysChat(fc.texts, fc.chat, TID_PK_COMBAT_END);
    return false;
  };

  const end = (r: Run): void => {
    if (r.timer !== null) stopTimer(r.timer);
    if (run === r) run = null;
  };

  const request = (kind: LogoutKind): LogoutStart => {
    if (ctx.machine.screen.s !== 'InGame') return 'refused';
    if (run) return 'running';
    if (!mayLogout()) return 'refused';
    if (immediate()) {
      finish(kind);
      return 'immediate';
    }
    const seconds = ctx.features.gameplay.timewait_close_s;
    // legacy-send: SendReqLeave
    requestLeave(ctx);
    const r: Run = { kind, deadline: now() + seconds * 1000, timer: null };
    run = r;
    const ui = fc.ui;
    if (ui) {
      const view: LogoutCountdownView = {
        secondsLeft: () => secondsLeftOf(r),
        text: (n) => formatTid(fc.texts, TID_PK_TIME_END, [n]),
        expired: () => {
          end(r);
          finish(kind);
        },
        closed: () => {
          end(r);
        },
      };
      ui.session.openCountdown(view, kind === 'logout');
    } else {
      // no window (tests, boot before B5): the same deadline on a plain timer
      r.timer = startTimer(() => {
        r.timer = null;
        end(r);
        finish(kind);
      }, seconds * 1000);
    }
    return 'countdown';
  };

  const cancel = (): void => {
    const r = run;
    if (!r) return;
    const w = fc.ui?.wm.get(APP_MESSAGE_TEXT) ?? null;
    if (w) {
      fc.ui?.wm.destroy(w); // `onDestroy` -> `closed()` -> `end(r)`
      fc.ui?.wm.flushDestroyed();
    }
    end(r);
  };

  // a kick, a lost connection or any other way out of the world ends the countdown (the window is modal: it must not stay)
  ctx.machine.subscribe((screen) => {
    if (run && screen.s !== 'InGame' && screen.s !== 'Reconnecting') cancel();
  });

  return {
    logout: () => request('logout'),
    quit: () => request('quit'),
    mayLogout,
    setAttackModeProbe(fn) {
      attackMode = fn;
    },
    cancel,
    get counting() {
      return run !== null;
    },
    get secondsLeft() {
      return run ? secondsLeftOf(run) : 0;
    },
  };
}
