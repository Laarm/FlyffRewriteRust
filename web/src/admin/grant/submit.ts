// Sending a grant (Plan/25 section 5.3, 12 rule 9): builds the request from the store, sends it with an `Idempotency-Key` and shows the
// answer. The key belongs to the request BODY: pressing the button again with the same basket, recipients and reason (after a timeout
// the operator could not tell whether it worked) sends the same key, so the server returns the stored report and grants nothing twice.
// Any change of the body is a new operation with a new key.
import type { StoreApi } from 'zustand/vanilla';
import { AdminApiError, isApiError, mutation, newIdempotencyKey, rest } from '../api/rest';
import { expectedFromMessage } from '../app/confirm';
import type { ActionResult, GrantOutcome, GrantReport, Reason } from '../gen/dto';
import { postGrant, validateItem } from './api';
import { canSendU64, dropItemJson } from './json';
import { blockedReason, requestOf, type GrantStore } from './store';
import { T } from './strings';

/** The keys of the attempts of one store: the same body keeps its key. */
const keyBooks = new WeakMap<object, Map<string, string>>();

/** The key for `body` in the book of `owner` (a new one for a body not seen before). */
export function keyFor(owner: object, body: unknown): string {
  let book = keyBooks.get(owner);
  if (!book) {
    book = new Map();
    keyBooks.set(owner, book);
  }
  const text = JSON.stringify(body);
  const known = book.get(text);
  if (known !== undefined) return known;
  const key = newIdempotencyKey();
  if (book.size >= 20) {
    const oldest = book.keys().next().value;
    if (oldest !== undefined) book.delete(oldest);
  }
  book.set(text, key);
  return key;
}

/** The reason of the form as the wire's `Reason`. */
export function reasonOf(preset: string | null, note: string): Reason {
  const text = note.trim();
  return { ...(preset !== null ? { preset } : {}), ...(text !== '' ? { text } : {}) };
}

/** How a grant went, per recipient. */
export interface ReportSummary {
  readonly delivered: number;
  readonly mailed: number;
  readonly pending: number;
  readonly failed: readonly {
    readonly name: string;
    readonly id: number;
    readonly message: string;
  }[];
}

/** Counts the outcomes of a report. */
export function summarize(report: GrantReport): ReportSummary {
  let delivered = 0;
  let mailed = 0;
  let pending = 0;
  const failed: { name: string; id: number; message: string }[] = [];
  report.results.forEach(([id, outcome], i) => {
    const name = report.names[i] ?? `#${String(id)}`;
    const o: GrantOutcome = outcome;
    if ('delivered' in o) delivered += 1;
    else if ('mailed' in o) mailed += 1;
    else if ('pending' in o) pending += 1;
    else failed.push({ name, id, message: o.failed.message || o.failed.code });
  });
  return { delivered, mailed, pending, failed };
}

/** Sends the grant of the store. Never throws: the outcome is in the store (`report`, `error`, `confirmFor`). */
export async function submitGrant(api: StoreApi<GrantStore>): Promise<GrantReport | null> {
  const s = api.getState();
  if (s.busy) return null;
  const blocked = blockedReason(s);
  if (blocked !== null) {
    s.setError(T.blocked[blocked as keyof typeof T.blocked]);
    return null;
  }
  s.setBusy(true);
  s.setError(null);
  s.setReport(null);
  try {
    const body = requestOf(s);
    const reason = reasonOf(s.preset, s.note);
    const confirm = s.confirmFor === null ? undefined : s.confirm.trim();
    const key = keyFor(api, [body, reason, confirm]);
    const report = await postGrant(body, reason, confirm, key);
    s.setReport(report);
    const sum = summarize(report);
    if (sum.failed.length < report.results.length) s.clearBasket();
    return report;
  } catch (error) {
    if (isApiError(error, 'confirm_required')) {
      s.askConfirm(expectedFromMessage(error.message) ?? 'CONFIRM');
    } else {
      s.setError(error instanceof Error ? error.message : String(error));
    }
    return null;
  } finally {
    s.setBusy(false);
  }
}

/** Puts every line of the basket on the ground (`drop-item`, one drop per line); gold cannot lie on the ground. */
export async function submitDrop(api: StoreApi<GrantStore>): Promise<number> {
  const s = api.getState();
  if (s.busy) return 0;
  const blocked = blockedReason(s);
  if (blocked !== null) {
    s.setError(T.blocked[blocked as keyof typeof T.blocked]);
    return 0;
  }
  s.setBusy(true);
  s.setError(null);
  s.setDropped(null);
  let drops = 0;
  try {
    const reason = reasonOf(s.preset, s.note);
    for (const line of s.lines) {
      const checked = await validateItem(line.spec);
      if (!checked.ok || !checked.item) {
        throw new AdminApiError(
          400,
          'invalid',
          T.drop.invalidLine(
            checked.name || String(line.spec.item_id),
            checked.errors[0]?.message ?? '',
          ),
        );
      }
      if (!canSendU64(checked.item.random_option)) throw new Error(T.drop.browser);
      const body = {
        world: s.ground.world,
        layer: s.ground.layer,
        pos: [s.ground.x, s.ground.y, s.ground.z],
        item: dropItemJson(checked.item),
        ...(s.ground.owner ? { owner: s.ground.owner.id } : {}),
      };
      const key = keyFor(api, ['drop', line.id, body]);
      const result = await rest.post<ActionResult>(
        '/admin/api/actions/drop-item',
        mutation(body, reason),
        {
          idempotent: true,
          idempotencyKey: key,
        },
      );
      drops += result.spawned.length;
    }
    s.setDropped(T.drop.done(drops));
    s.clearBasket();
    return drops;
  } catch (error) {
    s.setError(error instanceof Error ? error.message : String(error));
    return drops;
  } finally {
    s.setBusy(false);
  }
}
