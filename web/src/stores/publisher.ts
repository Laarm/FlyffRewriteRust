// Publisher (Plan/40 section 6.4, ST5, D7): copies high-frequency values (HP/MP/FP, EXP, positions,
// cooldown clocks) from the entity table and live records into the Zustand stores at <= 10 Hz, so React
// never re-renders per frame. The loop calls `tick(nowMs)` once per iteration (step 11, section 10.2);
// in a hidden tab it is not ticked and `flushAll()` runs on return.

import type { StoreApi } from 'zustand/vanilla';
import { now } from '../platform/time';
import type { Unsubscribe } from '../platform/visibility';

/** Supported publication rates. */
export type PublishHz = 1 | 2 | 4 | 10;

export interface PublisherStats {
  /** Runs per publisher name (a run = the publisher function was called). */
  readonly runs: Readonly<Record<string, number>>;
  /** Exceptions thrown by publisher functions (they are logged and swallowed). */
  readonly errors: number;
}

export interface Publisher {
  /**
   * Registers `fn`, called at most `hz` times per second from `tick`. The first `tick` after
   * registration runs it immediately. Returns the unsubscribe function.
   */
  register(name: string, hz: PublishHz, fn: (nowMs: number) => void): Unsubscribe;
  /** Runs the publishers that are due at `nowMs` (once per loop iteration). */
  tick(nowMs: number): void;
  /** Runs every publisher now (visibility return, after a drain that changed screens). */
  flushAll(nowMs?: number): void;
  readonly stats: PublisherStats;
}

export interface PublisherOptions {
  /** Called with `(name, error)` when a publisher function throws. */
  readonly onError?: (name: string, error: unknown) => void;
}

interface Entry {
  readonly name: string;
  readonly periodMs: number;
  readonly fn: (nowMs: number) => void;
  lastRunMs: number;
}

/** Slack that lets six 60 Hz frames (99.99... ms) count as one 10 Hz period. */
const TOLERANCE_MS = 0.5;

export function createPublisher(options: PublisherOptions = {}): Publisher {
  const entries = new Set<Entry>();
  const runs: Record<string, number> = {};
  let errors = 0;

  const run = (e: Entry, nowMs: number): void => {
    e.lastRunMs = nowMs;
    runs[e.name] = (runs[e.name] ?? 0) + 1;
    try {
      e.fn(nowMs);
    } catch (error) {
      errors += 1;
      options.onError?.(e.name, error);
    }
  };

  return {
    register(name, hz, fn) {
      const entry: Entry = { name, periodMs: 1000 / hz, fn, lastRunMs: Number.NEGATIVE_INFINITY };
      entries.add(entry);
      return () => {
        entries.delete(entry);
      };
    },
    tick(nowMs) {
      for (const e of entries) {
        if (nowMs - e.lastRunMs >= e.periodMs - TOLERANCE_MS) run(e, nowMs);
      }
    },
    flushAll(nowMs = now()) {
      for (const e of entries) run(e, nowMs);
    },
    get stats() {
      return { runs: { ...runs }, errors };
    },
  };
}

/**
 * Applies `patch` to `store` only when at least one field differs (`Object.is`); returns whether
 * `setState` was called. This is the "compare, then publish on change" step of every publisher.
 */
export function publishIfChanged<S extends object>(store: StoreApi<S>, patch: Partial<S>): boolean {
  const current = store.getState();
  for (const key of Object.keys(patch) as (keyof S)[]) {
    if (!Object.is(current[key], patch[key])) {
      store.setState(patch);
      return true;
    }
  }
  return false;
}
