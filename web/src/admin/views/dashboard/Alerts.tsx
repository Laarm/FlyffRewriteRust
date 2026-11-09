// The dashboard's Alerts tab (Plan/25 section 9.1): the alerts that hold now, the events that fired or cleared since the last
// restart, and the thresholds the server evaluates every 10 seconds (`[admin.alerts]`).
import { Fragment } from 'react';
import { useNow } from '../../app/polling';
import { Badge, EmptyState, ErrorState, Skeleton, Table, type Column } from '../../components';
import {
  getAlerts,
  type Alert,
  type AlertEvent,
  type AlertsResponse,
  type Thresholds,
} from './api';
import { formatAgo, formatBytes, formatDateTime, formatDuration, formatMicros } from './format';
import { D } from './strings';
import { useSnapshot } from './useSnapshot';

/** The thresholds as readable lines (tested). */
export function thresholdLines(
  t: Thresholds,
): readonly { readonly label: string; readonly value: string }[] {
  return [
    {
      label: 'Tick p99',
      value: `above ${formatMicros(t.tick_p99_us)} for ${formatDuration(t.tick_p99_for_s)}`,
    },
    { label: 'Database', value: 'degraded (writes are failing)' },
    {
      label: 'Backup',
      value:
        t.backup_max_age_s === null
          ? 'off'
          : `newest older than ${formatDuration(t.backup_max_age_s)}`,
    },
    {
      label: 'Disk',
      value: t.disk_min_bytes === null ? 'off' : `less than ${formatBytes(t.disk_min_bytes)} free`,
    },
    { label: 'Audit', value: 'a write or an acknowledgement failed (held 10 minutes)' },
    {
      label: 'Remote staff logins',
      value: `more than ${String(t.failed_logins)} failures in ${formatDuration(t.failed_logins_window_s)}`,
    },
    { label: 'Remote admin', value: 'a session from another host over plain HTTP' },
  ];
}

export function severityTone(s: Alert['severity']): 'danger' | 'warn' {
  return s === 'critical' ? 'danger' : 'warn';
}

export function Alerts() {
  const snap = useSnapshot((signal) => getAlerts(signal), 'alerts', 10_000);
  const nowS = Math.floor(useNow(1000) / 1000);
  const data: AlertsResponse | null = snap.data;
  if (snap.error !== null && data === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (data === null) return <Skeleton lines={5} />;

  const activeColumns: Column<Alert>[] = [
    {
      id: 'severity',
      header: 'Level',
      cell: (a) => <Badge tone={severityTone(a.severity)}>{a.severity}</Badge>,
      text: (a) => a.severity,
      width: '90px',
    },
    { id: 'title', header: 'Alert', cell: (a) => a.title, text: (a) => a.title },
    { id: 'detail', header: 'Detail', cell: (a) => a.detail, text: (a) => a.detail },
    {
      id: 'since',
      header: 'Since',
      cell: (a) => <span title={formatDateTime(a.since)}>{formatAgo(a.since, nowS)}</span>,
      text: (a) => a.since,
      width: '120px',
    },
  ];
  const recentColumns: Column<AlertEvent>[] = [
    {
      id: 'ts',
      header: 'Time',
      cell: (e) => formatDateTime(e.ts),
      text: (e) => e.ts,
      width: '170px',
    },
    {
      id: 'kind',
      header: 'Event',
      cell: (e) => <Badge tone={e.kind === 'fired' ? 'warn' : 'ok'}>{e.kind}</Badge>,
      text: (e) => e.kind,
      width: '90px',
    },
    { id: 'title', header: 'Alert', cell: (e) => e.title, text: (e) => e.title },
    { id: 'detail', header: 'Detail', cell: (e) => e.detail, text: (e) => e.detail },
  ];

  return (
    <div className="adm-stack">
      {data.active.length === 0 ? (
        <EmptyState icon="check" title={D.noAlerts} hint={D.noAlertsHint} />
      ) : (
        <Table
          label="Active alerts"
          columns={activeColumns}
          rows={data.active}
          rowKey={(a) => a.id}
        />
      )}
      <section aria-label={D.recentAlerts}>
        <h3 className="adm-h3">{D.recentAlerts}</h3>
        {data.recent.length === 0 ? (
          <p className="adm-muted">{D.noRecentAlerts}</p>
        ) : (
          <Table
            label={D.recentAlerts}
            columns={recentColumns}
            rows={data.recent}
            rowKey={(e) => `${String(e.ts)}-${e.id}-${e.kind}`}
          />
        )}
      </section>
      <section aria-label={D.thresholds}>
        <h3 className="adm-h3">{D.thresholds}</h3>
        <dl className="adm-dl">
          {thresholdLines(data.thresholds).map((l) => (
            <Fragment key={l.label}>
              <dt>{l.label}</dt>
              <dd>{l.value}</dd>
            </Fragment>
          ))}
        </dl>
        <p className="adm-muted">{D.thresholdsHint}</p>
      </section>
    </div>
  );
}
