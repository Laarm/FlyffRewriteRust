// The JS watchdog (Plan/40 section 12.1, WP-CLIENT-13, F-010): global error hooks, the loop heartbeat and the stuck-state
// timers. `main.ts` installs it first (it statically imports only `platform/*` and this file), before any other code
// is loaded, so a failing chunk is still reported; `attach()` binds it to the recovery ladder, the loop and the
// screen state machine once the context exists.
//
//   * uncaught errors and unhandled rejections are counted: 3 within 10 s request L3; a `WebAssembly.RuntimeError`
//     is a wasm trap and is always L3 (the instance is dead);
//   * `securitypolicyviolation` is only logged; `webglcontextlost` is forwarded by 41: no restore within 10 s is L3;
//   * heartbeat: while the tab is visible and the state is not `Fatal`/`Reloading`, a 1 s check of `loop.heartbeat`:
//     older than 3 s means the rAF loop died: restart it (L1); a second restart within 30 s is L3;
//   * stuck states: `Connecting` > 30 s closes the socket and reconnects; `Entering` > 75 s is the `enteringTimeout`
//     event; `Loading` > 60 s is the gate timeout (Plan/40 section 9.8 rule 6); `Leaving` > 30 s is L2.

import { log as defaultLog, type Logger } from '../platform/log';
import { now as platformNow } from '../platform/time';
import {
  isHidden as platformIsHidden,
  onVisibility,
  type Unsubscribe,
} from '../platform/visibility';
import type { ScreenName, ScreenState } from '../stores/screen';
import type { Recovery } from './recovery';

export const UNCAUGHT_WINDOW_MS = 10_000;
export const UNCAUGHT_LIMIT = 3;
export const HEARTBEAT_CHECK_MS = 1000;
export const HEARTBEAT_STALE_MS = 3000;
export const LOOP_RESTART_WINDOW_MS = 30_000;
export const WEBGL_RESTORE_MS = 10_000;

/** How long each state may last before the watchdog acts (Plan/40 section 12.1). */
export const STUCK_MS: Readonly<Partial<Record<ScreenName, number>>> = {
  Connecting: 30_000,
  Entering: 75_000,
  Loading: 60_000,
  Leaving: 30_000,
};

/** The slice of `window` the watchdog listens to. */
export interface WatchdogTarget {
  addEventListener(type: string, listener: (event: never) => void): void;
  removeEventListener(type: string, listener: (event: never) => void): void;
}

export interface WatchdogOptions {
  readonly target?: WatchdogTarget | null;
  readonly log?: Pick<Logger, 'info' | 'warn' | 'error'>;
  readonly now?: () => number;
  readonly timers?: {
    setTimeout(cb: () => void, ms: number): unknown;
    clearTimeout(id: unknown): void;
    setInterval(cb: () => void, ms: number): unknown;
    clearInterval(id: unknown): void;
  };
  readonly isHidden?: () => boolean;
  readonly subscribeVisibility?: (cb: (hidden: boolean) => void) => Unsubscribe;
}

/** What `attach` binds the watchdog to. */
export interface WatchdogBindings {
  readonly recovery: Pick<Recovery, 'l1' | 'l2' | 'l3'>;
  readonly loop: { readonly running: boolean; readonly heartbeat: number; restart(): void };
  readonly machine: {
    readonly screen: ScreenState;
    subscribe(fn: (screen: ScreenState, previous: ScreenState) => void): Unsubscribe;
    dispatch(event: { type: 'enteringTimeout' } | { type: 'gateTimeout'; now: number }): unknown;
  };
  /** The connection abandons a session that never became established (default: nothing). */
  readonly onConnectingStuck?: () => void;
}

export interface WatchdogStats {
  readonly uncaught: number;
  readonly rejections: number;
  readonly wasmTraps: number;
  readonly loopRestarts: number;
  readonly cspViolations: number;
  readonly stuck: number;
}

export interface Watchdog {
  /** Binds the recovery ladder, the loop and the machine; idempotent per call (a second attach replaces the first). */
  attach(bindings: WatchdogBindings): void;
  /** `webglcontextlost` forwarded from 41: no restore within 10 s is L3. */
  webglContextLost(): void;
  webglContextRestored(): void;
  readonly stats: WatchdogStats;
  /** Removes every listener and timer. */
  dispose(): void;
}

/** `true` for an error that kills the wasm instance (`WebAssembly.RuntimeError`). */
export function isWasmTrap(error: unknown): boolean {
  return typeof WebAssembly === 'object' && error instanceof WebAssembly.RuntimeError;
}

function describe(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

export function createWatchdog(options: WatchdogOptions = {}): Watchdog {
  const log = options.log ?? defaultLog;
  const now = options.now ?? platformNow;
  const timers = options.timers ?? {
    setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
    clearTimeout: (id) => {
      globalThis.clearTimeout(id as number);
    },
    setInterval: (cb, ms) => globalThis.setInterval(cb, ms),
    clearInterval: (id) => {
      globalThis.clearInterval(id as number);
    },
  };
  const isHidden = options.isHidden ?? platformIsHidden;
  const target = options.target === undefined ? defaultTarget() : options.target;

  const counts = {
    uncaught: 0,
    rejections: 0,
    wasmTraps: 0,
    loopRestarts: 0,
    cspViolations: 0,
    stuck: 0,
  };
  let bound: WatchdogBindings | null = null;
  const errorTimes: number[] = [];
  const restartTimes: number[] = [];
  let pendingTrap: string | null = null;
  let heartbeatTimer: unknown = null;
  let stuckTimer: unknown = null;
  let glTimer: unknown = null;
  let offScreen: Unsubscribe | null = null;
  let offVisibility: Unsubscribe | null = null;
  let ignoreHeartbeatUntil = 0;
  const listeners: [string, (event: never) => void][] = [];

  const uncaught = (kind: 'error' | 'rejection', error: unknown, message: string): void => {
    if (kind === 'error') counts.uncaught += 1;
    else counts.rejections += 1;
    log.error(
      'watchdog',
      `${kind === 'error' ? 'uncaught error' : 'unhandled rejection'}: ${message}`,
    );
    if (isWasmTrap(error)) {
      counts.wasmTraps += 1;
      if (bound) bound.recovery.l3('wasm trap');
      else pendingTrap = 'wasm trap';
      return;
    }
    const t = now();
    errorTimes.push(t);
    while (errorTimes.length > 0 && t - (errorTimes[0] ?? t) >= UNCAUGHT_WINDOW_MS)
      errorTimes.shift();
    if (errorTimes.length >= UNCAUGHT_LIMIT && bound) {
      errorTimes.length = 0;
      bound.recovery.l3(`${String(UNCAUGHT_LIMIT)} uncaught errors within 10 s`);
    }
  };

  const listen = (type: string, fn: (event: never) => void): void => {
    if (!target) return;
    target.addEventListener(type, fn);
    listeners.push([type, fn]);
  };
  listen('error', (e: { message?: string; error?: unknown }) => {
    uncaught('error', e.error, e.message ?? describe(e.error));
  });
  listen('unhandledrejection', (e: { reason?: unknown }) => {
    uncaught('rejection', e.reason, describe(e.reason));
  });
  listen('securitypolicyviolation', (e: { violatedDirective?: string; blockedURI?: string }) => {
    counts.cspViolations += 1;
    log.warn('watchdog', `CSP violation: ${e.violatedDirective ?? '?'} ${e.blockedURI ?? ''}`);
  });

  // ---- loop heartbeat -------------------------------------------------------------------------------------

  const screenName = (): ScreenName | null => bound?.machine.screen.s ?? null;

  const checkHeartbeat = (): void => {
    if (!bound || isHidden()) return;
    const name = screenName();
    if (name === 'Fatal' || name === 'Reloading' || name === 'Boot') return;
    const t = now();
    if (t < ignoreHeartbeatUntil || !bound.loop.running) return;
    if (t - bound.loop.heartbeat <= HEARTBEAT_STALE_MS) return;
    counts.loopRestarts += 1;
    restartTimes.push(t);
    while (restartTimes.length > 0 && t - (restartTimes[0] ?? t) >= LOOP_RESTART_WINDOW_MS) {
      restartTimes.shift();
    }
    if (restartTimes.length >= 2) {
      restartTimes.length = 0;
      bound.recovery.l3('the game loop died twice within 30 s');
      return;
    }
    bound.recovery.l1(
      `game loop restarted (no frame for ${String(Math.round(t - bound.loop.heartbeat))} ms)`,
    );
    bound.loop.restart();
    ignoreHeartbeatUntil = t + HEARTBEAT_CHECK_MS;
  };

  // ---- stuck states ---------------------------------------------------------------------------------------

  const armStuck = (screen: ScreenState): void => {
    if (stuckTimer !== null) timers.clearTimeout(stuckTimer);
    stuckTimer = null;
    const limit = STUCK_MS[screen.s];
    if (limit === undefined || !bound) return;
    const b = bound;
    stuckTimer = timers.setTimeout(() => {
      stuckTimer = null;
      if (b.machine.screen !== screen) return; // moved on
      counts.stuck += 1;
      log.warn('watchdog', `stuck in ${screen.s} for ${String(limit)} ms`);
      const action: Partial<Record<ScreenName, () => void>> = {
        Connecting: () => {
          b.onConnectingStuck?.();
        },
        Entering: () => {
          b.machine.dispatch({ type: 'enteringTimeout' });
        },
        Loading: () => {
          b.machine.dispatch({ type: 'gateTimeout', now: now() });
        },
        Leaving: () => {
          b.recovery.l2('Leaving did not finish within 30 s');
        },
      };
      action[screen.s]?.();
    }, limit);
  };

  // ---- attach / dispose -----------------------------------------------------------------------------------

  const subscribeVisibility =
    options.subscribeVisibility ??
    ((cb: (hidden: boolean) => void): Unsubscribe =>
      onVisibility((e) => {
        if (e === 'hidden') cb(true);
        else if (e === 'visible') cb(false);
      }));

  return {
    attach(bindings) {
      offScreen?.();
      offVisibility?.();
      if (heartbeatTimer !== null) timers.clearInterval(heartbeatTimer);
      bound = bindings;
      if (pendingTrap !== null) {
        const reason = pendingTrap;
        pendingTrap = null;
        bindings.recovery.l3(reason);
      }
      offScreen = bindings.machine.subscribe(() => {
        armStuck(bindings.machine.screen);
      });
      armStuck(bindings.machine.screen);
      offVisibility = subscribeVisibility((hidden) => {
        // after the tab comes back the loop needs a frame or two before its heartbeat is fresh
        if (!hidden) ignoreHeartbeatUntil = now() + 2 * HEARTBEAT_CHECK_MS;
      });
      heartbeatTimer = timers.setInterval(checkHeartbeat, HEARTBEAT_CHECK_MS);
      if (errorTimes.length >= UNCAUGHT_LIMIT) {
        errorTimes.length = 0;
        bindings.recovery.l3(`${String(UNCAUGHT_LIMIT)} uncaught errors within 10 s`);
      }
    },

    webglContextLost() {
      log.warn('watchdog', 'WebGL context lost');
      if (glTimer !== null) timers.clearTimeout(glTimer);
      glTimer = timers.setTimeout(() => {
        glTimer = null;
        bound?.recovery.l3('the WebGL context was not restored within 10 s');
      }, WEBGL_RESTORE_MS);
    },

    webglContextRestored() {
      log.info('watchdog', 'WebGL context restored');
      if (glTimer !== null) timers.clearTimeout(glTimer);
      glTimer = null;
    },

    get stats(): WatchdogStats {
      return { ...counts };
    },

    dispose() {
      for (const [type, fn] of listeners) target?.removeEventListener(type, fn);
      listeners.length = 0;
      offScreen?.();
      offVisibility?.();
      for (const id of [heartbeatTimer]) if (id !== null) timers.clearInterval(id);
      for (const id of [stuckTimer, glTimer]) if (id !== null) timers.clearTimeout(id);
      heartbeatTimer = stuckTimer = glTimer = null;
      bound = null;
    },
  };
}

function defaultTarget(): WatchdogTarget | null {
  return typeof window === 'undefined' ? null : (window as unknown as WatchdogTarget);
}

let installed: Watchdog | null = null;

/** Installs the page-wide watchdog (idempotent); `main.ts` calls this first. */
export function install(): Watchdog {
  installed ??= createWatchdog();
  return installed;
}

/** The installed watchdog, if any (the boot attaches it to the context). */
export function installedWatchdog(): Watchdog | null {
  return installed;
}
