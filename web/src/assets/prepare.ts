// "Preparing assets n %" (Plan/40 section 9.9, WP-CLIENT-09 and -12, PL-130, Plan/24 section 12).
//
// The server builds derived assets lazily (collision chunks, PNG conversions, ...). Three inputs describe that
// work and are merged into `sessionStore.prepare`:
//   1. `bootstrap.prepare` (the `PrepareProgress` of 24 section 12.2 at page load),
//   2. `AssetsPreparing` pushes on the WebSocket (24 section 12.4: 2 per second at most, `finished` ends a wave),
//   3. the client's OWN requests that wait for the server (a request pending, or sleeping on a 503 `Retry-After`).
// While at least one request of the client is pending, `GET /api/prepare` is polled every 2 s (a request can
// hang for 30 s while the server builds, and the push only reaches sessions in the states of 24 section 12.4).
//
// `PrepareState.active` is the flag the UI shows: a wave is active AND one of the client's own requests has been
// pending for more than 1 s (24 section 12.4; login is never blocked, the flag only decorates a screen).

import type { StoreApi } from 'zustand/vanilla';
import type { AssetsPreparing } from '../gen/proto/flyff/v1/session_pb';
import type { PrepareState, SessionState } from '../stores/session';

/** The JSON of `GET /api/prepare` and of `bootstrap.prepare` (24 section 12.2). */
export interface PrepareProgress {
  readonly active: boolean;
  readonly phase?: string;
  readonly percent: number;
  readonly done?: number;
  readonly total?: number;
  readonly kind: string;
  readonly current: string;
  readonly waiting?: number;
}

/** A request must be pending this long before the UI shows the progress (24 section 12.4). */
export const SHOW_AFTER_MS = 1000;
/** Poll interval of `GET /api/prepare` while a request is pending. */
export const POLL_MS = 2000;

export interface PrepareTrackerOptions {
  /** `sessionStore`. */
  readonly store: Pick<StoreApi<SessionState>, 'getState' | 'setState'>;
  /** `GET /api/prepare`; `null` = unavailable. Never rejects into the tracker (errors are ignored). */
  readonly fetchProgress?: () => Promise<PrepareProgress | null>;
  readonly now?: () => number;
  readonly timers?: {
    setTimeout(cb: () => void, ms: number): unknown;
    clearTimeout(id: unknown): void;
  };
  readonly showAfterMs?: number;
  readonly pollMs?: number;
}

export interface PrepareTracker {
  /** The `bootstrap.prepare` object. */
  fromBootstrap(progress: PrepareProgress): void;
  /** An `AssetsPreparing` push. */
  onPush(msg: Pick<AssetsPreparing, 'kind' | 'done' | 'total' | 'finished' | 'current'>): void;
  /** `FetchScheduler.onPending`: one of the client's own requests started or stopped being pending. */
  setPending(key: string, pending: boolean): void;
  /** `FetchScheduler.onWaiting`: a request sleeps on a 503 `Retry-After` (counted as pending as well). */
  setWaiting(key: string, waiting: boolean): void;
  /** Own requests currently pending / sleeping on a build. */
  readonly pendingCount: number;
  readonly waitingCount: number;
  dispose(): void;
}

function percentOf(done: number, total: number): number {
  return total > 0 ? Math.min(100, Math.floor((done * 100) / total)) : 0;
}

export function createPrepareTracker(options: PrepareTrackerOptions): PrepareTracker {
  const now = options.now ?? (() => performance.now());
  const timers = options.timers ?? {
    setTimeout: (cb, ms) => globalThis.setTimeout(cb, ms),
    clearTimeout: (id) => {
      globalThis.clearTimeout(id as number);
    },
  };
  const showAfter = options.showAfterMs ?? SHOW_AFTER_MS;
  const pollMs = options.pollMs ?? POLL_MS;

  let wave: { active: boolean; percent: number; kind: string; current: string } = {
    active: false,
    percent: 0,
    kind: '',
    current: '',
  };
  const pending = new Map<string, number>();
  const waiting = new Set<string>();
  let showTimer: unknown = null;
  let pollTimer: unknown = null;
  let polling = false;
  let disposed = false;
  let shownSince: number | null = null;

  const oldestPending = (): number | null => {
    let oldest: number | null = null;
    for (const t of pending.values()) if (oldest === null || t < oldest) oldest = t;
    return oldest;
  };

  /** Recomputes the UI-visible state and the timers. */
  const recompute = (): void => {
    if (disposed) return;
    const oldest = oldestPending();
    const t = now();
    const aged = oldest !== null && t - oldest >= showAfter;
    const active = wave.active && aged;
    if (active) shownSince ??= t;
    else shownSince = null;
    const prev = options.store.getState().prepare;
    const next: PrepareState | null =
      wave.active || prev !== null
        ? {
            active,
            percent: wave.percent,
            kind: wave.kind,
            current: wave.current,
            since: shownSince ?? 0,
          }
        : null;
    if (
      (next === null && prev === null) ||
      (next !== null &&
        prev !== null &&
        prev.active === next.active &&
        prev.percent === next.percent &&
        prev.kind === next.kind &&
        prev.current === next.current)
    ) {
      // nothing the UI shows changed
    } else {
      options.store.setState({ prepare: next });
    }
    // a wave that is active while requests are young: look again when the oldest one turns 1 s
    if (showTimer !== null) {
      timers.clearTimeout(showTimer);
      showTimer = null;
    }
    if (wave.active && oldest !== null && !aged) {
      showTimer = timers.setTimeout(
        () => {
          showTimer = null;
          recompute();
        },
        Math.max(0, showAfter - (t - oldest)),
      );
    }
    schedulePoll();
  };

  const schedulePoll = (): void => {
    const f = options.fetchProgress;
    if (!f || disposed) return;
    if (pending.size === 0) {
      if (pollTimer !== null) timers.clearTimeout(pollTimer);
      pollTimer = null;
      return;
    }
    if (pollTimer !== null || polling) return;
    pollTimer = timers.setTimeout(() => {
      pollTimer = null;
      polling = true;
      f()
        .then((p) => {
          if (p) applyProgress(p);
        })
        .catch(() => undefined)
        .finally(() => {
          polling = false;
          schedulePoll();
        });
    }, pollMs);
  };

  const applyProgress = (p: PrepareProgress): void => {
    wave = { active: p.active, percent: p.percent, kind: p.kind, current: p.current };
    recompute();
  };

  return {
    fromBootstrap: applyProgress,
    onPush(msg) {
      wave = {
        active: !msg.finished,
        percent: msg.finished ? 100 : percentOf(msg.done, msg.total),
        kind: msg.kind,
        current: msg.current,
      };
      recompute();
    },
    setPending(key, isPending) {
      if (isPending) {
        if (!pending.has(key)) pending.set(key, now());
      } else {
        pending.delete(key);
        waiting.delete(key);
      }
      recompute();
    },
    setWaiting(key, isWaiting) {
      if (isWaiting) waiting.add(key);
      else waiting.delete(key);
    },
    get pendingCount() {
      return pending.size;
    },
    get waitingCount() {
      return waiting.size;
    },
    dispose() {
      disposed = true;
      if (showTimer !== null) timers.clearTimeout(showTimer);
      if (pollTimer !== null) timers.clearTimeout(pollTimer);
      showTimer = null;
      pollTimer = null;
    },
  };
}
