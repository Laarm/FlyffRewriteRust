// The dashboard's Activity tab (Plan/25 section 9.1): the latest admin actions and the notable events of the server (bans,
// authority changes, disbands, start and stop), newest first. The feed lives in memory; the audit log has the full record.
import { useState } from 'react';
import {
  Badge,
  Checkbox,
  EmptyState,
  ErrorState,
  Skeleton,
  Table,
  type Column,
} from '../../components';
import { getActivity, type ActivityItem } from './api';
import { formatDateTime } from './format';
import { D } from './strings';
import { useSnapshot } from './useSnapshot';

export function stateTone(state: string | null): 'ok' | 'danger' | 'neutral' {
  return state === 'done' ? 'ok' : state === 'failed' ? 'danger' : 'neutral';
}

export function Activity() {
  const [adminOnly, setAdminOnly] = useState(false);
  const snap = useSnapshot(
    (signal) => getActivity(adminOnly ? 'admin' : 'all', 50, signal),
    adminOnly ? 'admin' : 'all',
    10_000,
  );
  const columns: Column<ActivityItem>[] = [
    {
      id: 'ts',
      header: 'Time',
      cell: (a) => formatDateTime(Math.floor(a.ts_ms / 1000)),
      text: (a) => a.ts_ms,
      width: '170px',
    },
    {
      id: 'ev',
      header: 'Event',
      cell: (a) => <code>{a.ev}</code>,
      text: (a) => a.ev,
      filterable: true,
      width: '170px',
    },
    {
      id: 'actor',
      header: 'By',
      cell: (a) => a.actor ?? '-',
      text: (a) => a.actor ?? '',
      filterable: true,
      width: '140px',
    },
    {
      id: 'state',
      header: 'Result',
      cell: (a) => (a.state ? <Badge tone={stateTone(a.state)}>{a.state}</Badge> : '-'),
      text: (a) => a.state ?? '',
      width: '90px',
    },
    {
      id: 'summary',
      header: 'What',
      cell: (a) => a.summary,
      text: (a) => a.summary,
      filterable: true,
    },
  ];
  if (snap.error !== null && snap.data === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (snap.data === null) return <Skeleton lines={5} />;
  return (
    <div className="adm-stack">
      <div className="adm-row">
        <Checkbox
          label="Admin actions only"
          checked={adminOnly}
          onChange={(e) => {
            setAdminOnly(e.target.checked);
          }}
        />
      </div>
      {snap.data.items.length === 0 ? (
        <EmptyState icon="scroll" title={D.activityEmpty} hint={D.activityHint} />
      ) : (
        <Table
          label="Latest admin actions"
          columns={columns}
          rows={snap.data.items}
          rowKey={(a) => `${String(a.ts_ms)}-${a.ev}-${a.summary}`}
          exportName="activity"
        />
      )}
    </div>
  );
}
