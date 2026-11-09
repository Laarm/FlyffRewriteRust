// Server -> Sessions & network (Plan/25 section 9.3; WP-ADMIN-26): the gateway sessions (lobby, character select, in the world)
// from `GET /admin/api/sessions` with a kick action, the avatars in the world, and the per-session network statistics, which are
// phase-gated on WP-OPS-18 (`GET /admin/api/netstats` answers `needs_feature` until then).
import { Fragment, useState } from 'react';
import { isApiError, rest } from '../../api/rest';
import { runConfirmed } from '../../app/mutate';
import {
  Badge,
  Button,
  ErrorState,
  NeedsFeature,
  Skeleton,
  Table,
  type Column,
} from '../../components';
import { useCan } from '../../stores/session';
import { useStatus } from '../../stores/status';
import { formatBytes, formatDuration } from '../dashboard/format';
import { useSnapshot } from '../dashboard/useSnapshot';
import type { GatewaySession, OnlineRow, SessionsResponse } from './api';
import { T } from './strings';

/** `Preset: text`, the reason as the older kick route takes it (a plain string). */
export function reasonText(
  reason: { preset?: string | null; text?: string | null } | undefined,
): string | undefined {
  if (!reason) return undefined;
  const preset = reason.preset?.trim() ?? '';
  const text = reason.text?.trim() ?? '';
  if (preset && text) return `${preset}: ${text}`;
  return preset || text || undefined;
}

export function stateTone(state: string): 'ok' | 'accent' | 'neutral' | 'warn' {
  switch (state) {
    case 'in_world':
      return 'ok';
    case 'char_select':
    case 'entering':
      return 'accent';
    case 'leaving':
      return 'warn';
    default:
      return 'neutral';
  }
}

function KickButton({
  account,
  label,
  onDone,
}: {
  readonly account: string;
  readonly label: string;
  readonly onDone: () => void;
}) {
  return (
    <Button
      size="sm"
      variant="danger"
      aria-label={`Kick ${label}`}
      onClick={(e) => {
        e.stopPropagation();
        void runConfirmed({
          title: T.sessions.kickTitle(label),
          confirmLabel: T.sessions.kick,
          danger: true,
          reason: 'required',
          run: async (given) => {
            const reason = reasonText(given.reason);
            const r = await rest.post<{ affected: number }>('/admin/api/kick', {
              account,
              ...(reason ? { reason } : {}),
            });
            return r.affected;
          },
          success: (n) => (n === 0 ? T.sessions.kickNobody : T.sessions.kickDone(n)),
        }).then(onDone);
      }}
    >
      {T.sessions.kick}
    </Button>
  );
}

function NetworkStats({ sessions }: { readonly sessions: readonly GatewaySession[] }) {
  const [picked, setPicked] = useState<number | null>(null);
  const [state, setState] = useState<
    | { readonly kind: 'idle' }
    | { readonly kind: 'needs'; readonly wp: string }
    | { readonly kind: 'rows'; readonly rows: readonly (readonly [string, string])[] }
    | { readonly kind: 'error'; readonly error: unknown }
  >({ kind: 'idle' });
  const load = async (id: number): Promise<void> => {
    setPicked(id);
    try {
      const body = await rest.get<Record<string, unknown>>('/admin/api/netstats', {
        query: { session: id },
      });
      setState({
        kind: 'rows',
        rows: Object.entries(body).map(
          ([k, v]) => [k, typeof v === 'string' ? v : JSON.stringify(v)] as const,
        ),
      });
    } catch (error) {
      if (isApiError(error, 'not_implemented') || isApiError(error, 'needs_feature')) {
        setState({
          kind: 'needs',
          wp: (isApiError(error) ? error.missingWp : null) ?? 'WP-OPS-18',
        });
      } else {
        setState({ kind: 'error', error });
      }
    }
  };
  return (
    <section aria-label={T.sessions.netTitle}>
      <h3 className="adm-h3">{T.sessions.netTitle}</h3>
      <p className="adm-muted">{T.sessions.netHint}</p>
      <div className="adm-row">
        {sessions.slice(0, 8).map((s) => (
          <Button
            key={s.id}
            size="sm"
            variant={picked === s.id ? 'primary' : 'secondary'}
            onClick={() => void load(s.id)}
          >
            {`#${String(s.id)}`}
          </Button>
        ))}
      </div>
      {state.kind === 'needs' && <NeedsFeature wp={state.wp} title="Network statistics" />}
      {state.kind === 'error' && <ErrorState error={state.error} />}
      {state.kind === 'rows' && (
        <dl className="adm-dl">
          {state.rows.map(([k, v]) => (
            <Fragment key={k}>
              <dt>{k}</dt>
              <dd>{v}</dd>
            </Fragment>
          ))}
        </dl>
      )}
      {state.kind === 'idle' && sessions.length === 0 && (
        <NeedsFeature wp="WP-OPS-18" title="Network statistics" />
      )}
    </section>
  );
}

export function Sessions() {
  const canKick = useCan('act.kick');
  const snap = useSnapshot(
    (signal) =>
      rest.get<SessionsResponse>('/admin/api/sessions', { query: { limit: 200 }, signal }),
    'sessions',
    5000,
  );
  const status = useStatus((s) => s.status);
  if (snap.error !== null && snap.data === null)
    return <ErrorState error={snap.error} onRetry={snap.reload} />;
  if (snap.data === null) return <Skeleton lines={6} />;

  const gateway: Column<GatewaySession>[] = [
    { id: 'id', header: '#', cell: (s) => s.id, text: (s) => s.id, sortable: true, width: '70px' },
    {
      id: 'state',
      header: 'State',
      cell: (s) => <Badge tone={stateTone(s.state)}>{s.state}</Badge>,
      text: (s) => s.state,
      filterable: true,
      width: '130px',
    },
    {
      id: 'account',
      header: 'Account',
      cell: (s) => s.account ?? '-',
      text: (s) => s.account ?? '',
      filterable: true,
    },
    {
      id: 'peer',
      header: 'Address',
      cell: (s) => s.peer ?? '',
      text: (s) => s.peer ?? '',
      pii: true,
    },
    {
      id: 'age',
      header: 'Age',
      cell: (s) => formatDuration(s.age_s),
      text: (s) => s.age_s,
      sortable: true,
      sortValue: (s) => s.age_s,
      width: '90px',
    },
    {
      id: 'queued',
      header: 'Queued',
      cell: (s) => formatBytes(s.queued_bytes),
      text: (s) => s.queued_bytes,
      sortable: true,
      sortValue: (s) => s.queued_bytes,
      align: 'right',
      width: '90px',
    },
    ...(canKick
      ? [
          {
            id: 'actions',
            header: '',
            cell: (s: GatewaySession) =>
              s.account ? (
                <KickButton account={s.account} label={s.account} onDone={snap.reload} />
              ) : null,
            width: '80px',
          } satisfies Column<GatewaySession>,
        ]
      : []),
  ];
  const world: Column<OnlineRow>[] = [
    {
      id: 'name',
      header: 'Character',
      cell: (r) => r.name,
      text: (r) => r.name,
      sortable: true,
      filterable: true,
    },
    {
      id: 'level',
      header: 'Lv',
      cell: (r) => r.level,
      text: (r) => r.level,
      sortable: true,
      sortValue: (r) => r.level,
      align: 'right',
      width: '60px',
    },
    {
      id: 'world',
      header: 'World',
      cell: (r) => `${String(r.world)}${r.layer ? `/${String(r.layer)}` : ''}`,
      text: (r) => r.world,
      sortable: true,
      sortValue: (r) => r.world,
      width: '80px',
    },
    {
      id: 'auth',
      header: 'Auth',
      cell: (r) => r.authority,
      text: (r) => r.authority,
      width: '60px',
    },
    { id: 'ip', header: 'Address', cell: (r) => r.ip ?? '', text: (r) => r.ip ?? '', pii: true },
    {
      id: 'lingering',
      header: '',
      cell: (r) => (r.lingering ? <Badge tone="warn">lingering</Badge> : null),
      text: (r) => (r.lingering ? 'lingering' : ''),
      width: '100px',
    },
    ...(canKick
      ? [
          {
            id: 'actions',
            header: '',
            cell: (r: OnlineRow) => (
              <KickButton account={r.name} label={r.name} onDone={snap.reload} />
            ),
            width: '80px',
          } satisfies Column<OnlineRow>,
        ]
      : []),
  ];

  return (
    <div className="adm-stack">
      {status && (
        <div className="adm-row" role="group" aria-label={T.sessions.states}>
          {Object.entries(status.sessions).map(([state, n]) => (
            <Badge key={state} tone={stateTone(state)}>{`${state}: ${String(n)}`}</Badge>
          ))}
        </div>
      )}
      <section aria-label={T.sessions.gateway}>
        <h3 className="adm-h3">{T.sessions.gateway}</h3>
        <Table
          label={T.sessions.gateway}
          columns={gateway}
          rows={snap.data.gateway}
          rowKey={(s) => String(s.id)}
          emptyTitle={T.sessions.emptyGateway}
          exportName="gateway-sessions"
        />
      </section>
      <section aria-label={T.sessions.inWorld}>
        <h3 className="adm-h3">{T.sessions.inWorld}</h3>
        <Table
          label={T.sessions.inWorld}
          columns={world}
          rows={snap.data.in_world}
          rowKey={(r) => String(r.character_id)}
          emptyTitle={T.sessions.emptyWorld}
          exportName="in-world"
        />
      </section>
      <NetworkStats sessions={snap.data.gateway} />
    </div>
  );
}
