// Items -> Pending (Plan/25 section 4.4, 4.5): this character's pending grants and pending removals, each with a Cancel (the rows
// that wait for the next login). They come from the grants and removals lists of the items backend, filtered by character.
import { useCallback, useEffect, useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { Button, ErrorState, Skeleton, Table, type Column } from '../../components';
import type { GrantRowDto, RemovalRowDto } from '../../gen/dto';
import { cancelPending, fetchGrants, fetchRemovals } from '../../grant/api';
import { useCan } from '../../stores/session';
import { useCharacter } from './context';
import { formatDateTime, formatNumber } from './format';
import { infoOf } from './ItemCell';
import { usePlayerGame } from './gamedata';
import { T } from './strings';

interface Pending {
  readonly grants: readonly GrantRowDto[];
  readonly removals: readonly RemovalRowDto[];
}

export function PendingPanel({ onChanged }: { readonly onChanged?: (n: number) => void }) {
  const { id, live } = useCharacter();
  const game = usePlayerGame();
  const canAct = useCan('act.items');
  const [data, setData] = useState<Pending | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  const reload = useCallback(() => {
    setRound((n) => n + 1);
  }, []);
  useEffect(() => {
    if (!canAct) return;
    const controller = new AbortController();
    const q = { state: 'open' as const, character: id };
    void Promise.all([
      fetchGrants(q, null, controller.signal),
      fetchRemovals(q, null, controller.signal),
    ]).then(
      ([g, r]) => {
        if (controller.signal.aborted) return;
        setData({ grants: g.items, removals: r.items });
        setError(null);
        onChanged?.(g.items.length + r.items.length);
      },
      (e: unknown) => {
        if (!controller.signal.aborted) setError(e);
      },
    );
    return () => {
      controller.abort();
    };
  }, [id, canAct, round, live.online, onChanged]);
  if (!canAct) return <p className="adm-muted">{T.items.pending.needsItems}</p>;
  if (error && !data) return <ErrorState error={error} onRetry={reload} />;
  if (!data) return <Skeleton lines={3} height={18} />;

  const cancel = (kind: 'grant' | 'removal', rowId: number, what: string): void => {
    void runConfirmed({
      title: T.items.pending.cancelTitle(what),
      confirmLabel: T.items.pending.cancelIt,
      danger: true,
      reason: 'optional',
      run: (given) => cancelPending(kind, rowId, given.reason),
      success: () => T.items.pending.cancelled,
    }).then((o) => {
      if (o.ok) reload();
    });
  };

  const grantColumns: Column<GrantRowDto>[] = [
    {
      id: 'what',
      header: T.items.pending.grants,
      cell: (g) =>
        g.kind === 'gold'
          ? T.items.pending.penya(formatNumber(g.gold))
          : `${infoOf(game.data, g.item?.item_id ?? 0).name}${(g.item?.count ?? 1) > 1 ? ` x${String(g.item?.count)}` : ''}`,
      text: (g) => (g.kind === 'gold' ? String(g.gold) : String(g.item?.item_id ?? '')),
    },
    { id: 'source', header: T.items.pending.source, cell: (g) => g.source, text: (g) => g.source },
    { id: 'note', header: T.items.pending.note, cell: (g) => g.note, text: (g) => g.note },
    {
      id: 'created',
      header: T.items.pending.created,
      cell: (g) => formatDateTime(g.created_at),
      text: (g) => g.created_at,
    },
    {
      id: 'cancel',
      header: '',
      cell: (g) => (
        <Button
          size="sm"
          onClick={() => {
            cancel('grant', g.id, `grant #${String(g.id)}`);
          }}
        >
          {T.items.pending.cancel}
        </Button>
      ),
    },
  ];
  const removalColumns: Column<RemovalRowDto>[] = [
    {
      id: 'what',
      header: T.items.pending.removals,
      cell: (r) =>
        r.item_id === 0
          ? T.items.pending.removePenya(formatNumber(r.count))
          : `${infoOf(game.data, r.item_id).name} x${String(r.count)}`,
      text: (r) => String(r.item_id),
    },
    { id: 'where', header: 'From', cell: (r) => r.location, text: (r) => r.location },
    {
      id: 'by',
      header: T.items.pending.source,
      cell: (r) => r.requested_by,
      text: (r) => r.requested_by,
    },
    { id: 'reason', header: T.items.pending.note, cell: (r) => r.reason, text: (r) => r.reason },
    {
      id: 'created',
      header: T.items.pending.created,
      cell: (r) => formatDateTime(r.created_at),
      text: (r) => r.created_at,
    },
    {
      id: 'cancel',
      header: '',
      cell: (r) => (
        <Button
          size="sm"
          onClick={() => {
            cancel('removal', r.id, `removal #${String(r.id)}`);
          }}
        >
          {T.items.pending.cancel}
        </Button>
      ),
    },
  ];
  if (data.grants.length === 0 && data.removals.length === 0) {
    return <p className="adm-muted">{T.items.pending.none}</p>;
  }
  return (
    <div className="plr-section">
      {data.grants.length > 0 ? (
        <Table
          label="Pending grants"
          columns={grantColumns}
          rows={data.grants}
          rowKey={(g) => String(g.id)}
        />
      ) : null}
      {data.removals.length > 0 ? (
        <Table
          label="Pending removals"
          columns={removalColumns}
          rows={data.removals}
          rowKey={(r) => String(r.id)}
        />
      ) : null}
    </div>
  );
}
