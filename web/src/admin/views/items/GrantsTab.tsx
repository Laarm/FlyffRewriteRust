// Items & Economy -> Grants & removals (Plan/25 section 5.4, 12 rule 10): the grants and removals that wait for an offline character,
// and the finished ones, with a cancel for every row that has not been applied. A cancel that races the delivery is never both: the
// server answers which one won, and the row shows it.
import { useEffect, useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import {
  Badge,
  Button,
  Field,
  NumberInput,
  Select,
  Table,
  type Column,
  type Tone,
} from '../../components';
import { useCan } from '../../stores/session';
import type { GrantRowDto, RemovalRowDto } from '../../gen/dto';
import { cancelPending, type PendingState } from '../../grant/api';
import { ensureGrantData, useGrantData } from '../../grant/data';
import { usePagedList } from '../../components';
import { T } from './strings';

const STATES: readonly PendingState[] = ['open', 'done', 'cancelled', 'all'];

function toneOf(state: string): Tone {
  switch (state) {
    case 'open':
      return 'accent';
    case 'done':
      return 'ok';
    case 'cancelled':
      return 'neutral';
    default:
      return 'warn';
  }
}

function when(unix: number): string {
  return new Date(unix * 1000).toISOString().slice(0, 16).replace('T', ' ');
}

const STATE_LABEL: Readonly<Record<string, string>> = T.grants.states;

function StateBadge({ state }: { readonly state: string }) {
  return <Badge tone={toneOf(state)}>{STATE_LABEL[state] ?? state}</Badge>;
}

export function GrantsTab() {
  const canCancel = useCan('act.items');
  const data = useGrantData((s) => s.data);
  const [state, setState] = useState<PendingState>('open');
  const [character, setCharacter] = useState<number | null>(null);
  const [days, setDays] = useState(30);
  useEffect(() => {
    void ensureGrantData();
  }, []);
  const query = {
    state,
    ...(character === null ? {} : { character }),
    ...(state === 'open' ? {} : { days }),
  };
  const grants = usePagedList<GrantRowDto>({ path: '/admin/api/grants', query });
  const removals = usePagedList<RemovalRowDto>({ path: '/admin/api/removals', query });
  const name = (id: number): string => {
    const n = data?.itemName(id);
    return n === undefined || n === '' ? `#${String(id)}` : n;
  };

  const cancel = (kind: 'grant' | 'removal', id: number, after: () => void): void => {
    void runConfirmed({
      title: kind === 'grant' ? T.grants.cancelGrantTitle(id) : T.grants.cancelRemovalTitle(id),
      message: T.grants.cancelBody,
      confirmLabel: T.grants.cancelIt,
      danger: true,
      reason: 'optional',
      run: (given) => cancelPending(kind, id, given.reason),
      success: (r) => T.grants.cancelled(r.state),
    }).then((o) => {
      if (o.ok) after();
    });
  };

  const grantColumns: Column<GrantRowDto>[] = [
    { id: 'id', header: T.grants.id, cell: (r) => r.id, text: (r) => r.id, align: 'right' },
    {
      id: 'created',
      header: T.grants.created,
      cell: (r) => when(r.created_at),
      text: (r) => r.created_at,
    },
    {
      id: 'who',
      header: T.grants.receiver,
      cell: (r) => r.character_name,
      text: (r) => r.character_name,
      filterable: true,
    },
    {
      id: 'what',
      header: T.grants.what,
      cell: (r) =>
        r.item ? T.grants.items(name(r.item.item_id), r.item.count) : T.grants.penya(r.gold),
      text: (r) => (r.item ? name(r.item.item_id) : 'penya'),
      filterable: true,
    },
    {
      id: 'source',
      header: T.grants.source,
      cell: (r) => r.source,
      text: (r) => r.source,
      filterable: true,
    },
    { id: 'note', header: T.grants.note, cell: (r) => r.note, text: (r) => r.note },
    {
      id: 'state',
      header: T.grants.status,
      cell: (r) => <StateBadge state={r.state} />,
      text: (r) => r.state,
    },
    {
      id: 'actions',
      header: '',
      cell: (r) =>
        r.state === 'open' && canCancel ? (
          <Button
            size="sm"
            variant="danger"
            aria-label={`${T.grants.cancel} #${String(r.id)}`}
            onClick={() => {
              cancel('grant', r.id, grants.reload);
            }}
          >
            {T.grants.cancel}
          </Button>
        ) : null,
    },
  ];
  const removalColumns: Column<RemovalRowDto>[] = [
    { id: 'id', header: T.grants.id, cell: (r) => r.id, text: (r) => r.id, align: 'right' },
    {
      id: 'created',
      header: T.grants.created,
      cell: (r) => when(r.created_at),
      text: (r) => r.created_at,
    },
    {
      id: 'who',
      header: T.grants.receiver,
      cell: (r) => r.character_name ?? T.grants.guildBank(r.guild_name),
      text: (r) => r.character_name ?? r.guild_name ?? '',
      filterable: true,
    },
    { id: 'where', header: T.grants.where, cell: (r) => r.location, text: (r) => r.location },
    {
      id: 'what',
      header: T.grants.what,
      cell: (r) =>
        r.item_id === 0 ? T.grants.penya(r.count) : T.grants.items(name(r.item_id), r.count),
      text: (r) => (r.item_id === 0 ? 'penya' : name(r.item_id)),
      filterable: true,
    },
    { id: 'by', header: T.grants.by, cell: (r) => r.requested_by, text: (r) => r.requested_by },
    { id: 'reason', header: T.grants.reason, cell: (r) => r.reason, text: (r) => r.reason },
    {
      id: 'state',
      header: T.grants.status,
      cell: (r) => (
        <>
          <StateBadge state={r.state} /> {T.grants.pieces(r.removed_count)}
        </>
      ),
      text: (r) => r.state,
    },
    {
      id: 'actions',
      header: '',
      cell: (r) =>
        r.state === 'open' && canCancel ? (
          <Button
            size="sm"
            variant="danger"
            aria-label={`${T.grants.cancel} #${String(r.id)}`}
            onClick={() => {
              cancel('removal', r.id, removals.reload);
            }}
          >
            {T.grants.cancel}
          </Button>
        ) : null,
    },
  ];

  return (
    <div className="items-tab">
      <div className="items-toolbar">
        <Field label={T.grants.state}>
          <Select
            value={state}
            onChange={(e) => {
              setState(STATES.find((s) => s === e.target.value) ?? 'open');
            }}
          >
            {STATES.map((s) => (
              <option key={s} value={s}>
                {T.grants.states[s]}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={T.grants.character}>
          <NumberInput
            min={1}
            value={character ?? ''}
            onChange={(e) => {
              const n = Math.trunc(Number(e.target.value));
              setCharacter(e.target.value === '' || !Number.isFinite(n) || n < 1 ? null : n);
            }}
          />
        </Field>
        {state !== 'open' && (
          <Field label={T.grants.days}>
            <NumberInput
              min={1}
              max={3650}
              value={days}
              onChange={(e) => {
                setDays(Math.min(3650, Math.max(1, Math.trunc(Number(e.target.value)) || 30)));
              }}
            />
          </Field>
        )}
        <span className="grant-hint">{T.grants.hint}</span>
      </div>
      <h3 className="grant-h">{T.grants.grantsTitle}</h3>
      <Table
        columns={grantColumns}
        rows={grants.rows}
        rowKey={(r) => String(r.id)}
        label={T.grants.grantsTitle}
        loading={grants.loading}
        error={grants.error}
        onRetry={grants.reload}
        emptyTitle={T.grants.empty}
        hasMore={grants.hasMore}
        loadingMore={grants.loadingMore}
        onLoadMore={grants.loadMore}
        exportName="pending-grants"
      />
      <h3 className="grant-h">{T.grants.removalsTitle}</h3>
      <Table
        columns={removalColumns}
        rows={removals.rows}
        rowKey={(r) => String(r.id)}
        label={T.grants.removalsTitle}
        loading={removals.loading}
        error={removals.error}
        onRetry={removals.reload}
        emptyTitle={T.grants.empty}
        hasMore={removals.hasMore}
        loadingMore={removals.loadingMore}
        onLoadMore={removals.loadMore}
        exportName="pending-removals"
      />
    </div>
  );
}
