// The audit timeline of one subject (Plan/25 section 4.3 "History", 4.4 "ownership history", 4.7 "Logins"): the records of the audit
// index (`GET /admin/api/audit?character=|account=|serial=`, WP-ADMIN-29) for a character, an account or an item serial, newest first,
// with a type filter, and a link to the same query in Audit -> Audit log. It needs `view.audit` (authority N).
import { useMemo, useState } from 'react';
import {
  Badge,
  Button,
  ChipGroup,
  EmptyState,
  ErrorState,
  Select,
  Skeleton,
  usePagedList,
} from '../../components';
import { navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import { GROUPS, type AuditRow, type Group } from '../audit/api';
import { formatDateTime } from './format';
import { T } from './strings';

export interface TimelineSubject {
  readonly character?: number;
  readonly account?: number;
  readonly serial?: number | string;
}

const RANGES = [
  { id: '24h', label: 'Last 24 hours', ms: 86_400_000 },
  { id: '7d', label: 'Last 7 days', ms: 7 * 86_400_000 },
  { id: '30d', label: 'Last 30 days', ms: 30 * 86_400_000 },
] as const;

/** The query of the timeline at `nowMs`. */
export function timelineQuery(
  subject: TimelineSubject,
  rangeMs: number,
  nowMs: number,
  groups: readonly Group[],
): Record<string, string | number> {
  const q: Record<string, string | number> = { from: nowMs - rangeMs };
  if (subject.character !== undefined) q['character'] = subject.character;
  if (subject.account !== undefined) q['account'] = subject.account;
  if (subject.serial !== undefined) q['serial'] = String(subject.serial);
  if (groups.length > 0 && groups.length < GROUPS.length) q['groups'] = groups.join(',');
  return q;
}

/** The deep link of the same query in the audit log. */
export function auditParams(
  subject: TimelineSubject,
  range: string,
  groups: readonly Group[],
): Record<string, string> {
  const p: Record<string, string> = { range };
  if (subject.character !== undefined) p['character'] = String(subject.character);
  if (subject.account !== undefined) p['account'] = String(subject.account);
  if (subject.serial !== undefined) p['serial'] = String(subject.serial);
  if (groups.length > 0) p['groups'] = groups.join(',');
  return p;
}

export function AuditTimeline({
  subject,
  groups: fixedGroups,
  filterable = true,
  empty,
}: {
  readonly subject: TimelineSubject;
  /** Only these groups (the account's logins are `account` events). */
  readonly groups?: readonly Group[];
  /** Offer the type chips. */
  readonly filterable?: boolean;
  readonly empty: string;
}) {
  const canAudit = useCan('view.audit');
  const [range, setRange] = useState<(typeof RANGES)[number]['id']>('30d');
  const [pick, setPick] = useState<Group | null>(null);
  // the window is fixed when the view or the range changes ("last 30 days" is relative to that moment)
  const [now] = useState(() => Date.now());
  const rangeMs = RANGES.find((r) => r.id === range)?.ms ?? RANGES[2].ms;
  const groups = useMemo<readonly Group[]>(
    () => (pick ? [pick] : (fixedGroups ?? [])),
    [pick, fixedGroups],
  );
  const query = useMemo(
    () => timelineQuery(subject, rangeMs, now, groups),
    [subject, rangeMs, now, groups],
  );
  const list = usePagedList<AuditRow>({
    path: '/admin/api/audit',
    query,
    enabled: canAudit,
    limit: 50,
  });
  if (!canAudit) return <p className="adm-muted">{T.history.needsAudit}</p>;
  return (
    <div className="plr-section" data-testid="audit-timeline">
      <div className="plr-bar-top">
        <Select
          aria-label="Range"
          value={range}
          onChange={(e) => {
            setRange(e.target.value as (typeof RANGES)[number]['id']);
          }}
        >
          {RANGES.map((r) => (
            <option key={r.id} value={r.id}>
              {r.label}
            </option>
          ))}
        </Select>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            navigate({
              category: 'audit',
              tab: 'log',
              params: auditParams(subject, range, groups),
            });
          }}
        >
          {T.history.openLog}
        </Button>
      </div>
      {filterable ? (
        <ChipGroup
          label="Type"
          chips={GROUPS.map((g) => ({ value: g, label: g }))}
          value={pick}
          onChange={setPick}
        />
      ) : null}
      {list.loading ? (
        <Skeleton lines={4} height={18} />
      ) : list.error && list.rows.length === 0 ? (
        <ErrorState error={list.error} onRetry={list.reload} />
      ) : list.rows.length === 0 ? (
        <EmptyState title={empty} />
      ) : (
        <ul className="plr-timeline">
          {list.rows.map((r) => (
            <li key={r.id} className="plr-timeline__row" data-ev={r.ev}>
              <time className="plr-timeline__time">{formatDateTime(Math.floor(r.ts / 1000))}</time>
              <Badge>{r.group}</Badge>
              <span className="plr-timeline__what">
                {r.summary}
                {r.reason ? <span className="adm-muted"> ({r.reason})</span> : null}
              </span>
              {r.actor ? <span className="adm-muted adm-small">{r.actor}</span> : null}
            </li>
          ))}
        </ul>
      )}
      {list.hasMore ? (
        <div>
          <Button loading={list.loadingMore} onClick={list.loadMore}>
            Load more
          </Button>
        </div>
      ) : null}
    </div>
  );
}
