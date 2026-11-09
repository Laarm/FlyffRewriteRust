// The asset layer of a context in one call (Plan/40 section 2.3 `ctx.assets`): scheduler, HTTP source, manager and
// the preparing-assets tracker, wired together. `app/boot.ts` and the dev contexts build it; tests build the pieces.

import type { StoreApi } from 'zustand/vanilla';
import type { SessionState } from '../stores/session';
import { FetchScheduler, type FetchSchedulerOptions } from './fetchScheduler';
import {
  createAssetManager,
  type AssetManager,
  type AssetManagerOptions,
  type DecodePool,
} from './manager';
import { createPrepareTracker, type PrepareProgress, type PrepareTracker } from './prepare';
import { HttpAssetSource } from './source';
import type { AssetCaps } from './types';

export interface AssetLayer {
  readonly scheduler: FetchScheduler;
  readonly source: HttpAssetSource;
  readonly manager: AssetManager;
  readonly prepare: PrepareTracker;
}

export interface AssetLayerOptions {
  readonly sessionStore: Pick<StoreApi<SessionState>, 'getState' | 'setState'>;
  readonly pool?: DecodePool | null;
  readonly caps?: AssetCaps;
  readonly log?: { warn(scope: string, message: string): void };
  readonly now?: () => number;
  /** Test seams of the scheduler (`fetch`, `concurrency`, `timers`, `random`). */
  readonly scheduler?: Pick<FetchSchedulerOptions, 'fetch' | 'concurrency' | 'timers' | 'random'>;
  readonly manager?: Pick<
    AssetManagerOptions,
    'cpuBudget' | 'gpuBudget' | 'cpuIdleMs' | 'gpuIdleMs' | 'uploads'
  >;
  /** `GET /api/prepare` (default: a `fetch` of that URL with `cache: 'no-store'`). */
  readonly fetchProgress?: () => Promise<PrepareProgress | null>;
}

/** `GET /api/prepare` over the global fetch. */
export async function fetchPrepareProgress(
  fetchFn: typeof fetch = fetch,
): Promise<PrepareProgress | null> {
  const res = await fetchFn('/api/prepare', { cache: 'no-store' });
  if (!res.ok) return null;
  return (await res.json()) as PrepareProgress;
}

export function createAssetLayer(options: AssetLayerOptions): AssetLayer {
  const prepare = createPrepareTracker({
    store: options.sessionStore,
    fetchProgress: options.fetchProgress ?? (() => fetchPrepareProgress(options.scheduler?.fetch)),
    ...(options.now ? { now: options.now } : {}),
    ...(options.scheduler?.timers ? { timers: options.scheduler.timers } : {}),
  });
  const scheduler = new FetchScheduler({
    ...options.scheduler,
    ...(options.log ? { log: options.log } : {}),
    onPending: (key, pending) => {
      prepare.setPending(key, pending);
    },
    onWaiting: (key, waiting) => {
      prepare.setWaiting(key, waiting);
    },
  });
  const source = new HttpAssetSource({ scheduler });
  const manager = createAssetManager({
    source,
    pool: options.pool ?? null,
    ...(options.caps ? { caps: options.caps } : {}),
    ...(options.now ? { now: options.now } : {}),
    ...(options.log ? { log: options.log } : {}),
    ...options.manager,
    sourceStats: () => ({
      inflight: scheduler.stats.inflight,
      queued: scheduler.stats.queued + scheduler.stats.waiting,
    }),
  });
  return { scheduler, source, manager, prepare };
}
