// The live status of the server for the top bar's pills (Plan/25 section 2.1). Until WP-ADMIN-06's `StatusSubscribe` exists the
// console polls `GET /admin/api/status` (every 5 s while the page is in use, app/activity.ts). Alerts (`GET /admin/api/alerts`, WP-ADMIN-25)
// show only once that route exists: a `needs_feature` answer hides the pill instead of faking a zero.
import { create } from 'zustand';
import { isApiError, rest } from '../api/rest';
import type { StatusTick } from '../../gen/proto/admin';

/** `GET /admin/api/status` (Plan/10 section 13.2): the game's snapshot plus build and health fields. */
export interface ServerStatus {
  readonly tick: number;
  readonly uptime_s: number;
  readonly in_world: number;
  readonly lobby: number;
  readonly tick_p50_us: number;
  readonly tick_p99_us: number;
  readonly db_degraded: boolean;
  readonly db_queue: number;
  readonly shutdown_in_s: number | null;
  readonly build: string;
  readonly proto_hash: string;
  readonly web_build: string;
  /** `running`, `starting`, `draining`, ... (`HealthState::name`). */
  readonly state: string;
  readonly players: number;
  readonly sessions: Readonly<Record<string, number>>;
  readonly running: boolean;
  /** Present once WP-AUTH-18 (maintenance mode) lands. */
  readonly maintenance?: boolean;
}

/** What a 1 Hz `StatusTick` of the live socket says (the fields the pills show; `at` is when it arrived). */
export interface LiveStatus {
  readonly in_world: number;
  readonly lobby: number;
  readonly tick_p50_us: number;
  readonly tick_p99_us: number;
  readonly db_degraded: boolean;
  readonly db_queue: number;
  readonly alerts: number;
  readonly maintenance: boolean;
  /** `null` when no shutdown is running. */
  readonly shutdown_in_s: number | null;
  readonly at: number;
}

/** A live tick older than this is not trusted: the pills fall back to the polled status. */
export const LIVE_STALE_MS = 10_000;

/** The `StatusTick` of the socket as the store keeps it (a `shutdown_in_s` of 0 means none). */
export function liveFromTick(t: StatusTick, at: number): LiveStatus {
  return {
    in_world: t.inWorld,
    lobby: t.lobby,
    tick_p50_us: t.tickP50Us,
    tick_p99_us: t.tickP99Us,
    db_degraded: t.dbDegraded,
    db_queue: t.dbQueue,
    alerts: t.alerts,
    maintenance: t.maintenance,
    shutdown_in_s: t.shutdownInS === 0 ? null : t.shutdownInS,
    at,
  };
}

interface StatusState {
  readonly status: ServerStatus | null;
  /** The last tick of the live socket (Plan/25 section 11.4 `StatusSubscribe`), `null` until one arrives. */
  readonly live: LiveStatus | null;
  readonly applyTick: (t: StatusTick) => void;
  /** The last poll failed (the server does not answer, or the session ended). */
  readonly error: string | null;
  readonly updatedAt: number;
  /** Number of active alerts; `null` while unknown or when the alerts route does not exist yet. */
  readonly alerts: number | null;
  readonly alertsAvailable: boolean;
  readonly refreshStatus: () => Promise<void>;
  readonly refreshAlerts: () => Promise<void>;
}

/** The number of alerts in an `/admin/api/alerts` answer (`{active}` since WP-ADMIN-25; an array, `{items}` or `{alerts}` also parse), `null` for anything else. */
export function countAlerts(body: unknown): number | null {
  if (Array.isArray(body)) return body.length;
  if (typeof body === 'object' && body !== null) {
    const o = body as Record<string, unknown>;
    for (const key of ['active', 'items', 'alerts']) {
      const v = o[key];
      if (Array.isArray(v)) return v.length;
    }
  }
  return null;
}

export const useStatus = create<StatusState>()((set, get) => ({
  status: null,
  live: null,
  applyTick(t) {
    set({ live: liveFromTick(t, Date.now()) });
  },
  error: null,
  updatedAt: 0,
  alerts: null,
  alertsAvailable: true,

  async refreshStatus() {
    try {
      const status = await rest.get<ServerStatus>('/admin/api/status');
      set({ status, error: null, updatedAt: Date.now() });
    } catch (error) {
      // an account without `view.dashboard` (a raised `[admin.capabilities]` letter) simply has no pills: that is not an outage
      if (isApiError(error, 'forbidden')) {
        set({ status: null, error: null });
        return;
      }
      set({ error: error instanceof Error ? error.message : String(error) });
    }
  },

  async refreshAlerts() {
    if (!get().alertsAvailable) return;
    try {
      const alerts = countAlerts(await rest.get<unknown>('/admin/api/alerts'));
      set({ alerts });
    } catch (error) {
      if (isApiError(error, 'needs_feature') || isApiError(error, 'forbidden')) {
        set({ alertsAvailable: false, alerts: null });
      }
    }
  },
}));

/** `3_100` microseconds as `3.1 ms`. */
export function formatMs(us: number): string {
  return `${(us / 1000).toFixed(1)} ms`;
}

/** `3725` seconds as `1h 2m`. */
export function formatUptime(seconds: number): string {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${String(d)}d ${String(h)}h`;
  if (h > 0) return `${String(h)}h ${String(m)}m`;
  return `${String(m)}m ${String(Math.floor(seconds % 60))}s`;
}
