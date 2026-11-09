// Audit -> My actions (Plan/25 section 2.5, 10.2; WP-ADMIN-29): the operator's last 50 admin actions, a `requested` record and its
// `done` / `failed` one folded into one line, each with an Undo button when the action has an inverse.
import { useMemo, useState } from 'react';
import { Badge, Button, Field, Select, Table, usePagedList, type Column } from '../../components';
import type { AuditRow } from './api';
import { dataOf } from './api';
import { collapseActions, type ActionEntry } from './collapse';
import { describeAction } from './describe';
import { RecordDetail, stateTone, SubjectLinks, formatStamp } from './Detail';
import { RANGE_MS } from './filters';
import { T } from './strings';
import { UndoButton, useUndone } from './Undo';

/** The most actions the list shows. */
export const LAST_ACTIONS = 50;

type MineRange = '24h' | '7d' | '30d';

const OPTIONS: readonly { readonly id: MineRange; readonly label: string }[] = [
  { id: '24h', label: 'Last 24 hours' },
  { id: '7d', label: 'Last 7 days' },
  { id: '30d', label: 'Last 30 days' },
];

export function MyActions() {
  const undone = useUndone();
  const [range, setRange] = useState<MineRange>('24h');
  // fixed when the operator looks (again): "24 hours" is relative to that moment, not to every render
  const [since, setSince] = useState(() => Date.now());
  const [selected, setSelected] = useState<string | null>(null);
  const list = usePagedList<AuditRow>({
    path: '/admin/api/audit',
    query: { mine: 1, groups: 'admin', from: since - RANGE_MS[range] },
    limit: 200,
  });
  const entries = useMemo(() => collapseActions(list.rows, LAST_ACTIONS), [list.rows]);
  const refresh = (): void => {
    setSince(Date.now());
  };

  const columns: Column<ActionEntry>[] = [
    {
      id: 'time',
      header: 'Time',
      cell: (e) => <span className="adm-nowrap">{formatStamp(e.ts)}</span>,
      text: (e) => formatStamp(e.ts),
      width: '190px',
    },
    {
      id: 'action',
      header: 'Action',
      cell: (e) => (
        <span className="adm-ellipsis" title={e.ev}>
          {describeAction(e.ev, dataOf(e.row), e.summary)}
        </span>
      ),
      text: (e) => describeAction(e.ev, dataOf(e.row), e.summary),
    },
    { id: 'subject', header: 'Subject', cell: (e) => <SubjectLinks row={e.row} />, width: '150px' },
    {
      id: 'result',
      header: 'Result',
      cell: (e) =>
        e.state === 'recorded' ? null : <Badge tone={stateTone(e.state)}>{e.state}</Badge>,
      text: (e) => e.state,
      width: '100px',
    },
    {
      id: 'reason',
      header: 'Reason',
      cell: (e) => e.reason ?? '',
      text: (e) => e.reason ?? '',
      width: '180px',
    },
    {
      id: 'undo',
      header: '',
      cell: (e) => <UndoButton entry={e} undone={undone} onUndone={refresh} />,
      width: '90px',
    },
  ];

  const entry = useMemo(() => entries.find((e) => e.key === selected) ?? null, [entries, selected]);

  return (
    <div className="adm-stack">
      <p className="adm-muted">{T.mine.intro}</p>
      <div className="adm-row">
        <Field label={T.mine.range}>
          <Select
            value={range}
            onChange={(e) => {
              setRange(e.target.value as MineRange);
              setSince(Date.now());
            }}
          >
            {OPTIONS.map((o) => (
              <option key={o.id} value={o.id}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>
        <Button variant="secondary" onClick={refresh}>
          {T.mine.refresh}
        </Button>
      </div>
      <Table
        label={T.mine.table}
        columns={columns}
        rows={entries}
        rowKey={(e) => e.key}
        loading={list.loading}
        error={list.error}
        onRetry={list.reload}
        emptyTitle={T.mine.empty}
        emptyHint={T.mine.emptyHint}
        hasMore={list.hasMore && entries.length < LAST_ACTIONS}
        loadingMore={list.loadingMore}
        onLoadMore={list.loadMore}
        onRowActivate={(e) => {
          setSelected(e.key);
        }}
        selectedKey={selected}
      />
      {entry !== null && (
        <RecordDetail
          row={entry.row}
          requested={entry.requested}
          missingOutcome={entry.state === 'requested'}
          onClose={() => {
            setSelected(null);
          }}
        />
      )}
    </div>
  );
}
