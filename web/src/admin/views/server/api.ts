// The server pages' view of the admin API (Plan/25 section 9.3 - 9.9; WP-ADMIN-26).

/** One gateway session (`GET /admin/api/sessions`, `gateway`). */
export interface GatewaySession {
  readonly id: number;
  readonly state: string;
  /** Left out without `view.pii`. */
  readonly peer?: string;
  readonly account: string | null;
  readonly age_s: number;
  readonly queued_bytes: number;
}

/** One avatar in the world (`in_world`). */
export interface OnlineRow {
  readonly session: number;
  readonly account_id: number;
  readonly character_id: number;
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly world: number;
  readonly layer: number;
  readonly pos: readonly number[];
  readonly authority: string;
  readonly ip?: string;
  readonly in_world_since: number;
  readonly lingering: boolean;
}

export interface SessionsResponse {
  readonly gateway: readonly GatewaySession[];
  readonly in_world: readonly OnlineRow[];
}

export type ConfigClass = 'live' | 'reload' | 'restart';
export type ConfigSource = 'default' | 'file' | 'env' | 'cli' | 'options' | 'override';
export type ConfigKind = 'bool' | 'int' | 'float' | 'str' | 'str_array' | 'table_array' | 'table';

/** One row of `GET /admin/api/config`. */
export interface ConfigRow {
  readonly key: string;
  readonly section: string;
  readonly class: ConfigClass;
  readonly editable: boolean;
  readonly kind: ConfigKind;
  readonly value: unknown;
  readonly default: unknown;
  readonly source: ConfigSource;
  readonly source_detail: string | null;
  readonly range: string;
  readonly doc: string;
  readonly owner: string;
  readonly snippet: string;
}

export interface ConfigResponse {
  readonly counts: Readonly<Record<ConfigClass, number>>;
  readonly overrides: number;
  readonly persist_overrides: boolean;
  readonly keys: readonly ConfigRow[];
}

/** What `POST /admin/api/reload` did, and what `GET /admin/api/reload` says it would do (WP-OPS-16). */
export interface ReloadReport {
  /** Keys whose new value is in force (the runtime keys the game took included). */
  readonly changed: readonly string[];
  /** Runtime keys whose file value differs but an override in force keeps winning. */
  readonly overridden: readonly string[];
  /** Changed keys that need a restart; they keep their running value. */
  readonly restart_required: readonly string[];
  /** Why the file was not (or not completely) applied. */
  readonly errors: readonly string[];
  /** Findings of the loader that do not stop a reload (retired keys, reserved keys). */
  readonly warnings: readonly string[];
}

/** One scheduler job (`GET /admin/api/jobs`). */
export interface Job {
  readonly name: string;
  readonly kind: 'periodic' | 'slot' | 'wall';
  readonly row: string;
  readonly owner: number;
  readonly schedule: string;
  readonly gate_open: boolean;
  readonly runnable: boolean;
  readonly runs: number;
  readonly last_tick: number | null;
  readonly last_run_unix_ms: number | null;
  readonly manual_runs: number;
  readonly last_manual_unix_ms: number | null;
  readonly next_run_unix_ms: number | null;
}

export interface JobsResponse {
  readonly now_unix_ms: number;
  readonly jobs: readonly Job[];
}

export interface Verified {
  readonly at: number;
  readonly ok: boolean;
  readonly detail: string;
}

export interface BackupFile {
  readonly name: string;
  readonly kind: string;
  readonly at: number;
  readonly size_bytes: number;
  readonly verified: Verified | null;
}

export interface BackupsResponse {
  readonly dir: string;
  readonly files: readonly BackupFile[];
  readonly free_bytes: number | null;
  readonly newest_age_s: number | null;
  readonly running: boolean;
  readonly last_ok_unix: number | null;
  readonly failures: number;
  readonly policy: Readonly<Record<string, number>>;
  readonly restore_command: string;
  readonly download_remote_allowed: boolean;
}

export interface MaintenanceResponse {
  readonly available: boolean;
  readonly needs: string;
  readonly enabled: boolean;
  readonly allow_ips: readonly string[];
  readonly supervisor: { readonly detected: boolean; readonly kind: string | null };
  readonly restart_available: boolean;
  readonly restart_pending: boolean;
}

export interface ConsoleReply {
  readonly lines: readonly string[];
  readonly result: 'ok' | 'error';
}
