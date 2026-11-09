// "My actions" (Plan/25 section 2.5, 10.1): an admin mutation writes a `requested` record before it is dispatched and a `done` or
// `failed` one after, so the log has two records for one action. This folds them into one entry per outcome. Pure; tested.
import type { AuditRow } from './api';

export type EntryState = 'done' | 'failed' | 'requested' | 'recorded';

/** One action of the operator. */
export interface ActionEntry {
  /** The id of the outcome record (of the `requested` one when no outcome was written). */
  readonly key: string;
  /** Unix milliseconds of the outcome. */
  readonly ts: number;
  readonly ev: string;
  readonly actor: string | null;
  /** `recorded`: a record that has no state of its own (a fact: `account.ban`). `requested`: no outcome was written. */
  readonly state: EntryState;
  readonly reason: string | null;
  readonly summary: string;
  readonly subject: AuditRow['subject'];
  /** The outcome record, or the `requested` one when that is all there is. */
  readonly row: AuditRow;
  readonly requested: AuditRow | null;
}

/** A `done` record without a `rid` belongs to the `requested` one of the same action when it follows within this time. */
export const PAIR_WINDOW_MS = 5 * 60_000;

function pairs(req: AuditRow, out: AuditRow): boolean {
  if (req.rid !== null && out.rid !== null) return req.rid === out.rid;
  return (
    req.rid === null &&
    out.rid === null &&
    req.ev === out.ev &&
    req.actor === out.actor &&
    out.ts >= req.ts &&
    out.ts - req.ts <= PAIR_WINDOW_MS
  );
}

/** The entry of one record on its own (the audit log lists records, not actions). */
export function entryOfRow(row: AuditRow): ActionEntry {
  const state =
    row.state === 'done' || row.state === 'failed' || row.state === 'requested'
      ? row.state
      : 'recorded';
  return entryOf(row, state, null);
}

function entryOf(row: AuditRow, state: EntryState, requested: AuditRow | null): ActionEntry {
  return {
    key: row.id,
    ts: row.ts,
    ev: row.ev,
    actor: row.actor,
    state,
    reason: row.reason ?? requested?.reason ?? null,
    summary: row.summary,
    subject: row.subject,
    row,
    requested,
  };
}

/**
 * The entries of `rows` (any order), newest first, at most `limit`. The first outcome after a `requested` record takes it; a
 * mutation with several outcomes (a teleport of three characters writes one `done` each) gives one entry per outcome.
 */
export function collapseActions(rows: readonly AuditRow[], limit = 50): ActionEntry[] {
  const oldestFirst = [...rows].sort((a, b) => a.ts - b.ts || a.id.localeCompare(b.id));
  const open: { row: AuditRow; used: boolean }[] = [];
  const entries: ActionEntry[] = [];
  for (const row of oldestFirst) {
    if (row.state === 'requested') {
      open.push({ row, used: false });
      continue;
    }
    if (row.state === 'done' || row.state === 'failed') {
      let match: { row: AuditRow; used: boolean } | null = null;
      for (let i = open.length - 1; i >= 0; i -= 1) {
        const o = open[i];
        if (o && !o.used && pairs(o.row, row)) {
          match = o;
          break;
        }
      }
      if (match) match.used = true;
      entries.push(entryOf(row, row.state, match?.row ?? null));
      continue;
    }
    entries.push(entryOf(row, 'recorded', null));
  }
  for (const o of open) if (!o.used) entries.push(entryOf(o.row, 'requested', null));
  entries.sort((a, b) => b.ts - a.ts || b.key.localeCompare(a.key));
  return entries.slice(0, limit);
}
