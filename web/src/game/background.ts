// Hidden-tab behaviour (Plan/40 section 10.5, PL-014, WP-CLIENT-10 loop side): when the tab is hidden rAF
// stops, so a background pump keeps the session alive: it drains the network, steps the sim (with the same
// backlog cap) and lets the move reporter and audio director keep running, but never renders, uploads or
// publishes. The pump is triggered by a timer (browsers throttle it to >= 1 s and, after minutes, to once a
// minute) AND by every inbound frame (network events are not throttled), rate-limited to one pump per
// 250 ms (<= 4 Hz). On return one immediate pump runs, remote movers snap, the clock is re-synced and the
// publishers flush; rAF resumes by itself.

import { now as platformNow } from '../platform/time';
import { onVisibility, type Unsubscribe, type VisibilityEvent } from '../platform/visibility';
import type { InboundQueue } from '../net/inbound';
import type { GameLoop } from './loop';

/** Opaque timer handle (a number in browsers, a Timeout object in Node). */
type TimerId = unknown;

/** Minimum time between two pumps (<= 4 Hz). */
export const PUMP_INTERVAL_MS = 250;

export interface BackgroundPumpDeps {
  readonly loop: Pick<GameLoop, 'iterate' | 'setHidden'>;
  readonly inbound: Pick<InboundQueue, 'onPush'>;
  /** `input.release()`: all keys and buttons up, pointer capture released (43). Resolved lazily. */
  readonly releaseInput?: () => void;
  /** `sim.resetRenderSmoothing()` on return: remote movers snap (ref/21 port note 7). */
  readonly resetRenderSmoothing?: () => void;
  /** `publisher.flushAll()` on return. */
  readonly flushPublishers?: () => void;
  /** Called on return: the connection sends `ServerTimeReq` at once (section 5.6). */
  readonly onReturn?: () => void;
  /** Called when the tab goes hidden / returns (the connection pauses and restarts its liveness timer). */
  readonly onHiddenChange?: (hidden: boolean) => void;
  readonly now?: () => number;
  readonly intervalMs?: number;
  readonly timers?: {
    setInterval(cb: () => void, ms: number): TimerId;
    clearInterval(id: TimerId): void;
  };
  /** Page lifecycle events (default: `platform/visibility`). */
  readonly subscribe?: (cb: (e: VisibilityEvent) => void) => Unsubscribe;
  readonly isHidden?: () => boolean;
}

export interface BackgroundPump {
  /** Starts listening to the page lifecycle (idempotent). */
  start(): void;
  stop(): void;
  /** Test hook `test.simulateHidden(on)`: behaves exactly like the browser event. */
  simulateHidden(on: boolean): void;
  readonly hidden: boolean;
  /** Pumps run while hidden. */
  readonly pumps: number;
}

export function createBackgroundPump(deps: BackgroundPumpDeps): BackgroundPump {
  const now = deps.now ?? platformNow;
  const intervalMs = deps.intervalMs ?? PUMP_INTERVAL_MS;
  const timers = deps.timers ?? {
    setInterval: (cb: () => void, ms: number) => globalThis.setInterval(cb, ms),
    clearInterval: (id: TimerId) => {
      globalThis.clearInterval(id as number);
    },
  };
  const subscribe = deps.subscribe ?? onVisibility;

  let hidden = false;
  let started = false;
  let timerId: TimerId = null;
  let unsubPush: Unsubscribe | null = null;
  let unsubVisibility: Unsubscribe | null = null;
  let lastPumpAt = Number.NEGATIVE_INFINITY;
  let pumps = 0;

  const pump = (): void => {
    const t = now();
    lastPumpAt = t;
    pumps += 1;
    deps.loop.iterate(t, false);
  };

  /** A pump if one is due (>= 250 ms since the last one). */
  const maybePump = (): void => {
    if (!hidden) return;
    if (now() - lastPumpAt < intervalMs - 0.5) return;
    pump();
  };

  const enterHidden = (): void => {
    if (hidden) return;
    hidden = true;
    deps.loop.setHidden(true);
    deps.releaseInput?.();
    timerId = timers.setInterval(maybePump, intervalMs);
    unsubPush = deps.inbound.onPush(maybePump);
    deps.onHiddenChange?.(true);
  };

  const leaveHidden = (): void => {
    if (!hidden) return;
    hidden = false;
    if (timerId !== null) timers.clearInterval(timerId);
    timerId = null;
    unsubPush?.();
    unsubPush = null;
    deps.loop.setHidden(false);
    deps.onHiddenChange?.(false);
    // one immediate pump, then the snap, the clock probe and the publishers (section 10.5 step 3)
    pump();
    deps.resetRenderSmoothing?.();
    deps.onReturn?.();
    deps.flushPublishers?.();
  };

  const onEvent = (e: VisibilityEvent): void => {
    if (e === 'hidden' || e === 'pagehide') enterHidden();
    else if (e === 'visible') leaveHidden();
  };

  return {
    start() {
      if (started) return;
      started = true;
      unsubVisibility = subscribe(onEvent);
      if (deps.isHidden?.() === true) enterHidden();
    },
    stop() {
      if (!started) return;
      started = false;
      unsubVisibility?.();
      unsubVisibility = null;
      if (hidden) {
        hidden = false;
        if (timerId !== null) timers.clearInterval(timerId);
        timerId = null;
        unsubPush?.();
        unsubPush = null;
        deps.loop.setHidden(false);
      }
    },
    simulateHidden(on) {
      if (on) enterHidden();
      else leaveHidden();
    },
    get hidden() {
      return hidden;
    },
    get pumps() {
      return pumps;
    },
  };
}
