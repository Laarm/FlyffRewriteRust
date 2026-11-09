// The audit pages' view of the admin API (Plan/25 section 10.2, 11.3; WP-ADMIN-29): the rows of `GET /admin/api/audit` and the
// small readers of a record's `data` object that the descriptions and the inverse-action catalogue share.

/** The ids a record is about (the index keys of `admin::audit_index`). */
export interface AuditSubject {
  readonly characters: readonly number[];
  readonly accounts: readonly number[];
  readonly serials: readonly number[];
}

/** One record of the audit log, as `GET /admin/api/audit` answers it (IPs and e-mail addresses are already removed without `view.pii`). */
export interface AuditRow {
  /** The cursor of the record: unique, and stable between calls. */
  readonly id: string;
  /** Unix milliseconds. */
  readonly ts: number;
  readonly ev: string;
  readonly group: string;
  readonly actor: string | null;
  /** `requested`, `done` or `failed` for an admin mutation; `null` for a record that has no state. */
  readonly state: string | null;
  /** Correlates the `requested` record of a mutation with its `done` / `failed` one. */
  readonly rid: string | null;
  readonly reason: string | null;
  readonly summary: string;
  readonly subject: AuditSubject;
  /** The whole JSONL line (`ts`, `ev`, `data`, ...). */
  readonly raw: Readonly<Record<string, unknown>>;
}

/** The event groups of the filter (`admin::audit_index::group_of`). */
export const GROUPS = [
  'account',
  'character',
  'item',
  'economy',
  'guild',
  'gm',
  'admin',
  'server',
] as const;

export type Group = (typeof GROUPS)[number];

export function isRecord(v: unknown): v is Readonly<Record<string, unknown>> {
  return v !== null && typeof v === 'object' && !Array.isArray(v);
}

/** The number in `v`, or `null` (a numeric string counts: ids travel both ways). */
export function num(v: unknown): number | null {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string' && /^-?\d+$/.test(v.trim())) return Number(v);
  return null;
}

export function str(v: unknown): string | null {
  return typeof v === 'string' ? v : null;
}

/** The `data` object of a record (`{}` for a record without one). */
export function dataOf(row: AuditRow): Readonly<Record<string, unknown>> {
  const d = row.raw['data'];
  return isRecord(d) ? d : {};
}

/** The numbers of an array value (anything else is left out). */
export function numbers(v: unknown): number[] {
  if (!Array.isArray(v)) return [];
  const out: number[] = [];
  for (const x of v as unknown[]) {
    const n = num(x);
    if (n !== null) out.push(n);
  }
  return out;
}
