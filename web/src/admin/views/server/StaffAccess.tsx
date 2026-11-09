// Server -> Staff & access (Plan/25 section 9.4, `view.staff`): the staff accounts (resolved authority L or higher, `[gm]` grants
// included) with their effective capabilities, the last game login, the TOTP state and the open admin sessions; the owner sessions; and
// the read-only capability matrix with the `[admin.capabilities]` overrides highlighted. Actions (`act.accounts`): end a session, set
// an authority. Rank protection is the server's: the buttons only hide what the session cannot do.
import { Fragment, useCallback, useEffect, useState } from 'react';
import { mutation, rest } from '../../api/rest';
import { runConfirmed } from '../../app/mutate';
import { Badge, Button, ErrorState, Skeleton, Table, type Column } from '../../components';
import type { AdminSessionRow, CapRow, StaffRow, StaffView, TotpResetAnswer } from '../../gen/dto';
import { useCan } from '../../stores/session';
import { changeAuthority } from '../players/accountActions';
import { authorityName, formatAgo, formatDateTime } from '../players/format';
import { T } from '../players/strings';
import '../players/players.css';

const S = T.staff;

export async function loadStaff(): Promise<StaffView> {
  return await rest.get<StaffView>('/admin/api/staff');
}

/** Removes the second factor of another account (a lost phone) after a confirmation with a mandatory reason; `true` when it went through. */
async function resetTotp(account: string): Promise<boolean> {
  const out = await runConfirmed({
    title: S.resetTotpTitle(account),
    message: S.resetTotpHint,
    confirmLabel: S.resetTotp,
    danger: true,
    reason: 'required',
    run: (given) =>
      rest.delete<TotpResetAnswer>(
        `/admin/api/accounts/${encodeURIComponent(account)}/totp`,
        mutation({}, given.reason),
      ),
    success: S.resetTotpDone(account),
  });
  return out.ok;
}

function SessionsTable({
  rows,
  canEnd,
  onEnded,
  now,
}: {
  readonly rows: readonly AdminSessionRow[];
  readonly canEnd: boolean;
  readonly onEnded: () => void;
  readonly now: number;
}) {
  const columns: Column<AdminSessionRow>[] = [
    {
      id: 'identity',
      header: S.sessionColumns.identity,
      cell: (s) => s.identity,
      text: (s) => s.identity,
    },
    {
      id: 'authority',
      header: S.sessionColumns.authority,
      cell: (s) => authorityName(s.authority),
      text: (s) => s.authority,
    },
    {
      id: 'ip',
      header: S.sessionColumns.ip,
      pii: true,
      cell: (s) => s.ip ?? '-',
      text: (s) => s.ip ?? '',
    },
    {
      id: 'remote',
      header: S.sessionColumns.remote,
      cell: (s) => (s.remote ? <Badge tone="warn">{S.remote}</Badge> : <Badge>{S.local}</Badge>),
      text: (s) => (s.remote ? 'remote' : 'local'),
    },
    {
      id: 'since',
      header: S.sessionColumns.since,
      cell: (s) => formatDateTime(s.created_at),
      text: (s) => s.created_at,
    },
    {
      id: 'seen',
      header: S.sessionColumns.lastSeen,
      cell: (s) => formatAgo(now, s.last_seen_at),
      text: (s) => s.last_seen_at,
    },
    {
      id: 'end',
      header: '',
      cell: (s) =>
        canEnd ? (
          <Button
            size="sm"
            variant="danger"
            onClick={() => {
              void runConfirmed({
                title: S.endTitle(s.identity),
                confirmLabel: S.end,
                danger: true,
                run: () => rest.delete(`/admin/api/admin-sessions/${encodeURIComponent(s.id)}`),
                success: S.ended,
              }).then((o) => {
                if (o.ok) onEnded();
              });
            }}
          >
            {S.end}
          </Button>
        ) : null,
    },
  ];
  return (
    <Table
      label={S.sessions}
      columns={columns}
      rows={rows}
      rowKey={(s) => s.id}
      emptyTitle={S.noSessions}
    />
  );
}

function Matrix({ rows }: { readonly rows: readonly CapRow[] }) {
  const letters = ['L', 'M', 'N', 'P'] as const;
  const rank = (l: string): number => 'DFGHJLMNOPZ'.indexOf(l);
  return (
    <div className="adm-table__scroll" role="region" aria-label={S.matrix} tabIndex={0}>
      <table className="plr-matrix" data-testid="cap-matrix">
        <thead>
          <tr>
            <th scope="col">{S.capability}</th>
            {letters.map((l) => (
              <th key={l} scope="col">
                {l}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.cap} className={r.min !== r.default_min ? 'is-override' : ''}>
              <th scope="row" title={r.grants}>
                <code>{r.cap}</code>
                {r.min !== r.default_min ? (
                  <Badge tone="warn" title={`default ${r.default_min}`}>
                    {S.overrides}
                  </Badge>
                ) : null}
              </th>
              {letters.map((l) => (
                <td
                  key={l}
                  className={rank(l) >= rank(r.min) ? 'is-yes' : 'is-no'}
                  aria-label={rank(l) >= rank(r.min) ? 'yes' : 'no'}
                >
                  {rank(l) >= rank(r.min) ? '✓' : ''}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function StaffAccess() {
  const canAct = useCan('act.accounts');
  const [data, setData] = useState<StaffView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const reload = useCallback(() => {
    setRound((n) => n + 1);
  }, []);
  useEffect(() => {
    let live = true;
    loadStaff().then(
      (d) => {
        if (live) {
          setData(d);
          setError(null);
          setNow(Math.floor(Date.now() / 1000));
        }
      },
      (e: unknown) => {
        if (live) setError(e);
      },
    );
    return () => {
      live = false;
    };
  }, [round]);
  // the sessions change under an open page: read again every 15 s
  useEffect(() => {
    const id = setInterval(reload, 15_000);
    return () => {
      clearInterval(id);
    };
  }, [reload]);
  if (error && !data) return <ErrorState error={error} onRetry={reload} />;
  if (!data) return <Skeleton lines={6} height={18} />;

  const columns: Column<StaffRow>[] = [
    {
      id: 'account',
      header: S.columns.account,
      sortable: true,
      cell: (s) => (
        <span className="adm-row">
          <strong>{s.account}</strong>
          {s.must_change_password ? <Badge tone="warn">{S.mustChange}</Badge> : null}
        </span>
      ),
      text: (s) => s.account,
    },
    {
      id: 'authority',
      header: S.columns.authority,
      sortable: true,
      cell: (s) => (
        <span className="adm-row">
          <Badge tone="accent">{authorityName(s.authority)}</Badge>
          {s.stored_authority !== s.authority ? (
            <span className="adm-muted adm-small">
              {S.columns.stored}: {s.stored_authority}
            </span>
          ) : null}
        </span>
      ),
      text: (s) => s.authority,
    },
    {
      id: 'caps',
      header: S.columns.caps,
      cell: (s) => <span title={s.caps.join(', ')}>{S.effectiveCaps(s.caps.length)}</span>,
      text: (s) => s.caps.length,
    },
    {
      id: 'login',
      header: S.columns.lastLogin,
      sortable: true,
      cell: (s) => formatDateTime(s.last_login_at),
      text: (s) => s.last_login_at ?? 0,
    },
    {
      id: 'ip',
      header: S.sessionColumns.ip,
      pii: true,
      cell: (s) => s.last_login_ip ?? '-',
      text: (s) => s.last_login_ip ?? '',
    },
    {
      id: 'totp',
      header: S.columns.totp,
      cell: (s) =>
        s.totp_enabled ? <Badge tone="ok">{S.totpOn}</Badge> : <Badge>{S.totpOff}</Badge>,
      text: (s) => (s.totp_enabled ? 'on' : 'off'),
    },
    {
      id: 'sessions',
      header: S.columns.sessions,
      cell: (s) => (s.sessions.length === 0 ? S.noSessions : S.sessionsOf(s.sessions.length)),
      text: (s) => s.sessions.length,
    },
    {
      id: 'actions',
      header: '',
      cell: (s) =>
        canAct ? (
          <span className="adm-row">
            <Button
              size="sm"
              onClick={() => {
                void changeAuthority(s.account, s.stored_authority).then((ok) => {
                  if (ok) reload();
                });
              }}
            >
              {S.setAuthority}
            </Button>
            {s.totp_enabled ? (
              <Button
                size="sm"
                variant="danger"
                onClick={() => {
                  void resetTotp(s.account).then((ok) => {
                    if (ok) reload();
                  });
                }}
              >
                {S.resetTotp}
              </Button>
            ) : null}
          </span>
        ) : null,
    },
  ];
  const sessions = data.staff.flatMap((s) => s.sessions);
  return (
    <div className="plr-section" data-testid="staff-access">
      <h3>{S.title}</h3>
      <p className="adm-muted">{S.intro}</p>
      <Table
        label={S.title}
        columns={columns}
        rows={data.staff}
        rowKey={(s) => String(s.account_id)}
        emptyTitle={S.none}
      />
      <Fragment>
        <h3>{S.sessions}</h3>
        <SessionsTable rows={sessions} canEnd={canAct} onEnded={reload} now={now} />
        <h3>{S.ownerSessions}</h3>
        <p className="adm-muted adm-small">{S.ownerHint}</p>
        <SessionsTable rows={data.owner_sessions} canEnd={canAct} onEnded={reload} now={now} />
        <h3>{S.matrix}</h3>
        <p className="adm-muted adm-small">{S.matrixHint}</p>
        <Matrix rows={data.matrix} />
      </Fragment>
    </div>
  );
}
