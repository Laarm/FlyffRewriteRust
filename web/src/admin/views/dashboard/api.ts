// The dashboard's view of the admin API (Plan/25 section 9.1, 9.2; WP-ADMIN-25): `GET /admin/api/history`, `/alerts`, `/activity`.
import { rest } from '../../api/rest';

/** The columns of the metrics history (one array per field, one entry per sample). */
export interface Columns {
  readonly t: readonly number[];
  readonly in_world: readonly number[];
  readonly lobby: readonly number[];
  readonly tick_p50_us: readonly number[];
  readonly tick_p99_us: readonly number[];
  readonly db_queue: readonly number[];
  readonly rss_bytes: readonly number[];
  readonly bytes_in_s: readonly number[];
  readonly bytes_out_s: readonly number[];
  readonly ws_sessions: readonly number[];
  readonly admin_ws: readonly number[];
  readonly logins: readonly number[];
}

/** The cards that are not time series. */
export interface Current {
  readonly sampled_at: number;
  readonly started_unix: number;
  readonly rss_bytes: number;
  readonly disk_free_bytes: number | null;
  readonly cache_bytes: number;
  readonly cache_budget_bytes: number;
  readonly last_backup_unix: number | null;
  readonly backups: number;
  readonly db_wal_bytes: number;
  readonly db_checkpoints: number;
  readonly peak_in_world: number;
  readonly logins_last_hour: number;
  readonly maintenance: boolean;
}

export type Resolution = '10s' | '1m';

export interface History {
  readonly res: Resolution;
  readonly step_s: number;
  readonly span_s: number;
  readonly now: number;
  /** Seconds since the process started: the history is lost on a restart. */
  readonly since_start_s: number;
  readonly points: Readonly<Record<Resolution, number>>;
  readonly columns: Columns;
  readonly current: Current;
}

export type Severity = 'warn' | 'critical';

export interface Alert {
  readonly id: string;
  readonly severity: Severity;
  readonly title: string;
  readonly detail: string;
  readonly since: number;
}

export interface AlertEvent {
  readonly ts: number;
  readonly id: string;
  readonly kind: 'fired' | 'cleared';
  readonly title: string;
  readonly detail: string;
}

export interface Thresholds {
  readonly tick_p99_us: number;
  readonly tick_p99_for_s: number;
  readonly backup_max_age_s: number | null;
  readonly disk_min_bytes: number | null;
  readonly failed_logins: number;
  readonly failed_logins_window_s: number;
}

export interface AlertsResponse {
  readonly active: readonly Alert[];
  readonly recent: readonly AlertEvent[];
  readonly thresholds: Thresholds;
  readonly evaluated_at: number;
  readonly now: number;
}

export interface ActivityItem {
  readonly ts_ms: number;
  readonly ev: string;
  readonly actor: string | null;
  readonly state: string | null;
  readonly summary: string;
  readonly reason: string | null;
  readonly admin: boolean;
}

export interface ActivityResponse {
  readonly items: readonly ActivityItem[];
  readonly now_ms: number;
}

/** The time windows of the overview: the history switches to the minute resolution above two hours. */
export const SPANS = [
  { id: '1h', label: '1 hour', seconds: 3600 },
  { id: '2h', label: '2 hours', seconds: 7200 },
  { id: '6h', label: '6 hours', seconds: 21_600 },
  { id: '24h', label: '24 hours', seconds: 86_400 },
] as const;

export type SpanId = (typeof SPANS)[number]['id'];

export function spanSeconds(id: SpanId): number {
  return SPANS.find((s) => s.id === id)?.seconds ?? 3600;
}

export function getHistory(span: number, signal?: AbortSignal): Promise<History> {
  return rest.get<History>('/admin/api/history', {
    query: { span },
    ...(signal ? { signal } : {}),
  });
}

export function getAlerts(signal?: AbortSignal): Promise<AlertsResponse> {
  return rest.get<AlertsResponse>('/admin/api/alerts', signal ? { signal } : {});
}

export function getActivity(
  kind: 'all' | 'admin',
  limit: number,
  signal?: AbortSignal,
): Promise<ActivityResponse> {
  return rest.get<ActivityResponse>('/admin/api/activity', {
    query: { kind, limit },
    ...(signal ? { signal } : {}),
  });
}
