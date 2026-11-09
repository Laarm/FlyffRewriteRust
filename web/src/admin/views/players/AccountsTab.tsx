// Players -> Accounts (Plan/25 section 4.7): the accounts of 12's `GET /admin/api/accounts` (name substring, staff only, online only);
// the address column needs `view.pii`. A click opens the account drawer. "Create account" is the route of 12 section 5.5.
import { useEffect, useState } from 'react';
import { rest } from '../../api/rest';
import { withRecord } from '../../app/route';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Button,
  Checkbox,
  ErrorState,
  Field,
  Table,
  TextInput,
  type Column,
} from '../../components';
import { getRoute, navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import { createAccount } from './accountActions';
import { authorityName, formatDateTime } from './format';
import { useDebounced } from './hooks';
import { T } from './strings';

interface AccountRow {
  readonly id: number;
  readonly name: string;
  readonly authority: string;
  readonly created_at: number;
  readonly created_via: string;
  readonly last_login_at: number | null;
  readonly last_login_ip: string | null;
  /** Unix seconds, `permanent` or `null`. */
  readonly banned_until: number | 'permanent' | null;
  readonly characters: number;
  readonly online: 'offline' | 'char_select' | 'in_world';
}

export function openAccount(name: string): void {
  const route = getRoute() ?? { category: 'players', tab: 'accounts', params: {} };
  navigate(withRecord(route, { kind: 'account', id: name }));
}

export function AccountsTab({ route }: TabProps) {
  const [q, setQ] = useState('');
  const [staffOnly, setStaffOnly] = useState(false);
  const [onlineOnly, setOnlineOnly] = useState(false);
  const text = useDebounced(q.trim());
  const canCreate = useCan('act.accounts');
  const [rows, setRows] = useState<readonly AccountRow[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  useEffect(() => {
    let live = true;
    rest
      .get<AccountRow[]>('/admin/api/accounts', {
        query: {
          ...(text === '' ? {} : { name: text }),
          ...(staffOnly ? { gm: true } : {}),
          ...(onlineOnly ? { online: true } : {}),
          limit: 200,
        },
      })
      .then(
        (a) => {
          if (live) {
            setRows(a);
            setError(null);
          }
        },
        (e: unknown) => {
          if (live) setError(e);
        },
      );
    return () => {
      live = false;
    };
  }, [text, staffOnly, onlineOnly, round]);
  const columns: Column<AccountRow>[] = [
    {
      id: 'name',
      header: T.accounts.columns.name,
      sortable: true,
      filterable: true,
      cell: (a) => <strong>{a.name}</strong>,
      text: (a) => a.name,
    },
    {
      id: 'authority',
      header: T.accounts.columns.authority,
      sortable: true,
      cell: (a) =>
        a.authority === 'F' ? '-' : <Badge tone="accent">{authorityName(a.authority)}</Badge>,
      text: (a) => a.authority,
    },
    {
      id: 'state',
      header: T.accounts.columns.state,
      cell: (a) => (
        <span className="adm-row">
          {a.online === 'in_world' ? (
            <Badge tone="ok">{T.common.online}</Badge>
          ) : a.online === 'char_select' ? (
            <Badge tone="accent">lobby</Badge>
          ) : null}
          {a.banned_until !== null ? <Badge tone="danger">{T.accounts.state.banned}</Badge> : null}
        </span>
      ),
      text: (a) => `${a.online} ${a.banned_until === null ? '' : 'banned'}`,
    },
    {
      id: 'chars',
      header: 'Characters',
      align: 'right',
      sortable: true,
      cell: (a) => a.characters,
      text: (a) => a.characters,
    },
    {
      id: 'login',
      header: T.accounts.columns.lastLogin,
      sortable: true,
      cell: (a) => formatDateTime(a.last_login_at),
      text: (a) => a.last_login_at ?? 0,
    },
    {
      id: 'ip',
      header: T.accounts.columns.ip,
      pii: true,
      cell: (a) => a.last_login_ip ?? '-',
      text: (a) => a.last_login_ip ?? '',
    },
    {
      id: 'created',
      header: T.accounts.columns.created,
      sortable: true,
      cell: (a) => `${formatDateTime(a.created_at)} (${a.created_via})`,
      text: (a) => a.created_at,
    },
  ];
  return (
    <div className="plr-tab" data-testid="accounts-tab">
      <div className="plr-bar-top">
        <Field label={T.accounts.searchLabel} hint={T.accounts.searchHint}>
          <TextInput
            type="search"
            value={q}
            onChange={(e) => {
              setQ(e.target.value);
            }}
          />
        </Field>
        <Checkbox
          label={T.accounts.gmOnly}
          checked={staffOnly}
          onChange={(e) => {
            setStaffOnly(e.target.checked);
          }}
        />
        <Checkbox
          label={T.accounts.onlineOnly}
          checked={onlineOnly}
          onChange={(e) => {
            setOnlineOnly(e.target.checked);
          }}
        />
        {canCreate ? (
          <Button
            onClick={() => {
              void createAccount().then((ok) => {
                if (ok) setRound((n) => n + 1);
              });
            }}
          >
            {T.accounts.dialogs.create}
          </Button>
        ) : null}
      </div>
      {error && !rows ? (
        <ErrorState
          error={error}
          onRetry={() => {
            setRound((n) => n + 1);
          }}
        />
      ) : null}
      <Table
        label="Accounts"
        columns={columns}
        rows={rows ?? []}
        rowKey={(a) => a.name}
        loading={rows === null && !error}
        emptyTitle={T.accounts.empty}
        onRowActivate={(a) => {
          openAccount(a.name);
        }}
        selectedKey={route.params['a'] ?? null}
        exportName="accounts"
      />
    </div>
  );
}
