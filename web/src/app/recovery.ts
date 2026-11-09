// The recovery ladder (Plan/40 section 12.2, WP-CLIENT-13, F-010: legacy minidumps are dropped, the client heals):
//
//   L1 restart  the failed subsystem (the loop, a worker, an asset request): logged and recorded here
//   L2 resync   close with 4900 and run the reconnect policy with `Hello{reattach: wasInGame}`
//   L3 reload   write `flyff.recover`, reload the page; boot finds the token and re-attaches
//   L4 fatal    the fatal screen; 3 L3 within 120 s, an unsupported browser, a server that stays unreachable
//
// `reloadOnce(reason)` is the version guard (Plan/40 section 3.3): a reload with a cache-busting `?v=`, remembered in
// `sessionStorage['flyff.reload']`; the SAME reason twice within 60 s is not a stale cache but a broken deployment,
// so it ends in L4 with `TID_DIAG_0035` "Old version of client." Every level change is logged and kept for the
// probe (`__flyffDebug.recovery()`).

import { TID_DIAG_0035 } from '../gen/defs/text';
import type { Logger } from '../platform/log';
import type { SafeStorage } from '../platform/storage';
import type { FatalInfo } from '../stores/screen';
import type { MachineEvent } from './stateMachine';

export const RELOAD_KEY = 'flyff.reload';
export const RECOVER_KEY = 'flyff.recover';
/** The same `reloadOnce` reason within this window ends in L4. */
export const RELOAD_GUARD_MS = 60_000;
/** Three L3 within this window end in L4. */
export const L3_WINDOW_MS = 120_000;
/** How many L3 requests inside the window are answered with a reload (the third becomes L4). */
export const MAX_L3_IN_WINDOW = 2;
/** The built-in text of `TID_DIAG_0035` for the moment before the `texts` bundle is loaded. */
export const OLD_VERSION_TEXT = 'Old version of client.';

export interface RecoveryEntry {
  readonly level: number;
  readonly reason: string;
  /** `now()` (performance clock) at the transition. */
  readonly at: number;
}

export interface Recovery {
  /** The level transitions of this page load, oldest first (probe `recovery()`). */
  readonly history: readonly RecoveryEntry[];
  /** L1: a subsystem was restarted by its owner; recorded and logged. */
  l1(what: string): void;
  /** L2: close the socket with 4900 and reconnect with a re-attach. */
  l2(reason: string): void;
  /**
   * L3: reload the page (the ladder escalates to L4 when this is the third reload within 120 s).
   * Returns the level that was applied (3, or 4).
   */
  l3(reason: string): 3 | 4;
  /** L4: the fatal screen. */
  l4(reason: string, detail?: string): void;
  /**
   * The version guard (`'proto'`, `'build'`, `'data'`) and the boot-step guard (`'boot:<step>'`): reload with a cache
   * bust, once per reason per minute; then L4. `detail` names the failure on the fatal screen of a boot step.
   */
  reloadOnce(reason: string, detail?: string): void;
  /** The machine effect `reloadPage` (an L3 the machine already decided): does the reload itself. */
  performReload(reason: string): void;
}

export interface RecoveryDeps {
  /** The screen state machine (`dispatch` of `recoveryL3`/`recoveryL4`). */
  readonly dispatch: (event: MachineEvent) => unknown;
  readonly log: Pick<Logger, 'info' | 'warn' | 'error'>;
  /** Monotonic clock for the history. */
  readonly now: () => number;
  /** Wall clock: the memory in `sessionStorage` survives a reload, `performance.now()` does not. */
  readonly wall: () => number;
  /** `safeStorage.session`. */
  readonly storage: Pick<SafeStorage, 'get' | 'set' | 'remove'>;
  /** `location.reload()`. */
  readonly reload: () => void;
  /** `location.replace(url)` (the cache-busted reload of `reloadOnce`). */
  readonly replaceUrl: (url: string) => void;
  /** `location.pathname` and `location.search`. */
  readonly location: () => { pathname: string; search: string };
  /** `GETTEXT(tid)` when the texts bundle is loaded, else the fallback. */
  readonly text?: (tid: number, fallback: string) => string;
  /** The L2 action (the connection's resync); absent = only recorded. */
  readonly resync?: (reason: string) => void;
}

interface ReloadMemory {
  reason: string;
  at: number;
  count: number;
}

interface RecoverMemory {
  level: number;
  reason: string;
  at: number;
  count: number;
  /** Wall times of the L3 reloads of the last 120 s. */
  history: number[];
}

function readJson(storage: Pick<SafeStorage, 'get'>, key: string): unknown {
  const raw = storage.get(key);
  if (raw === null) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

const readReload = (storage: Pick<SafeStorage, 'get'>): ReloadMemory | null =>
  readJson(storage, RELOAD_KEY) as ReloadMemory | null;
const readRecover = (storage: Pick<SafeStorage, 'get'>): RecoverMemory | null =>
  readJson(storage, RECOVER_KEY) as RecoverMemory | null;

export function createRecovery(deps: RecoveryDeps): Recovery {
  const history: RecoveryEntry[] = [];
  const HISTORY_CAP = 100;

  const record = (level: number, reason: string): void => {
    history.push({ level, reason, at: deps.now() });
    if (history.length > HISTORY_CAP) history.shift();
    deps.log.warn('recovery', `L${String(level)}: ${reason}`);
  };

  const fatal = (info: FatalInfo): void => {
    deps.dispatch({ type: 'recoveryL4', error: info });
  };

  const recovery: Recovery = {
    get history() {
      return history;
    },

    l1(what) {
      record(1, what);
    },

    l2(reason) {
      record(2, reason);
      deps.resync?.(reason);
    },

    l3(reason) {
      const t = deps.wall();
      const memory = readRecover(deps.storage);
      const recent = (memory?.history ?? []).filter(
        (at) => typeof at === 'number' && t - at >= 0 && t - at < L3_WINDOW_MS,
      );
      if (recent.length >= MAX_L3_IN_WINDOW) {
        recovery.l4(
          `Recovery failed: ${reason}`,
          `${String(recent.length + 1)} reloads within 2 minutes`,
        );
        return 4;
      }
      const next: RecoverMemory = {
        level: 3,
        reason,
        at: t,
        count: (memory?.count ?? 0) + 1,
        history: [...recent, t],
      };
      deps.storage.set(RECOVER_KEY, JSON.stringify(next));
      record(3, reason);
      // the machine shows `Reloading` and runs its `reloadPage` effect, which calls `performReload`
      deps.dispatch({ type: 'recoveryL3', reason });
      return 3;
    },

    l4(reason, detail) {
      record(4, reason);
      fatal({ reason, ...(detail === undefined ? {} : { detail }) });
    },

    reloadOnce(reason, detail) {
      const t = deps.wall();
      const memory = readReload(deps.storage);
      if (memory?.reason === reason && t - memory.at >= 0 && t - memory.at < RELOAD_GUARD_MS) {
        // the same reload did not help: not a stale cache but a broken deployment (12 section 16.4)
        const isVersion = reason === 'proto' || reason === 'build' || reason === 'data';
        const text = isVersion
          ? (deps.text?.(TID_DIAG_0035, OLD_VERSION_TEXT) ?? OLD_VERSION_TEXT)
          : (detail ?? `Startup failed (${reason}).`);
        record(4, `reloadOnce(${reason}) repeated`);
        fatal({ reason: text, detail: `${reason} (repeated within 60 s)` });
        return;
      }
      const next: ReloadMemory = { reason, at: t, count: (memory?.count ?? 0) + 1 };
      deps.storage.set(RELOAD_KEY, JSON.stringify(next));
      record(3, `reloadOnce(${reason})`);
      const loc = deps.location();
      const search = new URLSearchParams(loc.search);
      search.set('v', String(t));
      deps.replaceUrl(`${loc.pathname}?${search.toString()}`);
    },

    performReload(reason) {
      deps.log.info('recovery', `reloading the page: ${reason}`);
      deps.reload();
    },
  };
  return recovery;
}
