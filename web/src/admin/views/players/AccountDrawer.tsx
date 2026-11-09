// The account drawer (Plan/25 section 4.7): name, authority, created (time, channel), last login (the address with `view.pii`), state,
// and the tabs Characters, Banks, Bans, Logins, Alts and Notes with the account actions (ban, unban, kick all sessions, reset password,
// set authority, end admin sessions). Banks are account-scoped: they are live through any online character of the account.
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNow } from '../../app/polling';
import type { DrawerBodyProps } from '../../app/views';
import {
  Badge,
  Button,
  ErrorState,
  Menu,
  Select,
  Skeleton,
  Table,
  Tabs,
  tabButtonId,
  tabPanelId,
  type Column,
  type MenuItem,
  type TabItem,
} from '../../components';
import type { AccountView, AltView, BanView, CharacterListItem } from '../../gen/dto';
import { useCan } from '../../stores/session';
import { navigate } from '../../stores/route';
import { Field } from '../../components';
import {
  banWholeAccount,
  changeAuthority,
  endSessionsOf,
  kickAccount,
  resetAccountPassword,
  unban,
} from './accountActions';
import { addIpBlock, fetchAccount, fetchAlts } from './api';
import { AuditTimeline } from './AuditTimeline';
import { BanksPanel } from './BanksPanel';
import { CharacterProvider } from './context';
import { runConfirmed } from '../../app/mutate';
import { StatusChips } from './CharactersTab';
import { authorityName, formatDateTime, formatSpan } from './format';
import { useItemsUi, useEquipment, useViewer } from './itemsShared';
import { JobIcon } from './JobIcon';
import { jobName } from './jobs';
import { useCharacterLive } from './live';
import { AddNote, NoteList } from './NotesPanel';
import { openCharacter } from './rowMenu';
import { T } from './strings';
import { TipProvider } from './Tip';
import { DURATION_CHIPS } from '../../app/confirm';

type TabId = 'characters' | 'banks' | 'bans' | 'logins' | 'alts' | 'notes';

function BansTable({ bans }: { readonly bans: readonly BanView[] }) {
  const columns: Column<BanView>[] = [
    {
      id: 'from',
      header: T.accounts.bans.from,
      cell: (b) => formatDateTime(b.created_at),
      text: (b) => b.created_at,
    },
    {
      id: 'until',
      header: T.accounts.bans.until,
      cell: (b) => (b.until === null ? T.accounts.bans.permanent : formatDateTime(b.until)),
      text: (b) => b.until ?? 0,
    },
    { id: 'reason', header: T.accounts.bans.reason, cell: (b) => b.reason, text: (b) => b.reason },
    { id: 'by', header: T.accounts.bans.by, cell: (b) => b.by, text: (b) => b.by },
    {
      id: 'state',
      header: '',
      cell: (b) =>
        b.active ? (
          <Badge tone="danger">{T.accounts.bans.active}</Badge>
        ) : b.lifted_at !== null ? (
          <Badge>{T.accounts.bans.liftedBy(b.lifted_by ?? '?', formatDateTime(b.lifted_at))}</Badge>
        ) : null,
    },
  ];
  return (
    <Table
      label="Bans"
      columns={columns}
      rows={bans}
      rowKey={(b) => String(b.id)}
      emptyTitle={T.accounts.bans.none}
    />
  );
}

function AltsPanel({ name }: { readonly name: string }) {
  const canPii = useCan('view.pii');
  const canBlock = useCan('act.ip_block');
  const [alts, setAlts] = useState<readonly AltView[] | null>(null);
  const [days, setDays] = useState(30);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    if (!canPii) return;
    let live = true;
    fetchAlts(name).then(
      (a) => {
        if (live) {
          setAlts(a.alts);
          setDays(a.days);
        }
      },
      (e: unknown) => {
        if (live) setError(e);
      },
    );
    return () => {
      live = false;
    };
  }, [name, canPii]);
  if (!canPii) return <p className="adm-muted">{T.accounts.alts.needsPii}</p>;
  if (error) return <ErrorState error={error} />;
  if (!alts) return <Skeleton lines={3} height={18} />;
  const blockIp = (ip: string): void => {
    void runConfirmed({
      title: T.moderation.ip.addTitle,
      message: <code>{ip}</code>,
      confirmLabel: T.moderation.ip.block,
      danger: true,
      reason: 'required',
      durations: DURATION_CHIPS,
      run: (given) => {
        const secs = given.duration?.seconds;
        return addIpBlock(
          ip,
          secs === null || secs === undefined ? null : Math.floor(Date.now() / 1000) + secs,
          given,
        );
      },
      success: T.moderation.ip.added(ip),
    });
  };
  const columns: Column<AltView>[] = [
    {
      id: 'name',
      header: T.accounts.columns.name,
      cell: (a) => <strong>{a.name}</strong>,
      text: (a) => a.name,
    },
    {
      id: 'ips',
      header: T.accounts.alts.shared,
      cell: (a) => (
        <span className="adm-stack adm-stack--tight">
          {a.shared_ips.map((ip) => (
            <span key={ip} className="adm-row">
              <code>{ip}</code>
              {canBlock ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    blockIp(ip);
                  }}
                >
                  {T.accounts.alts.blockIp}
                </Button>
              ) : null}
            </span>
          ))}
        </span>
      ),
      text: (a) => a.shared_ips.join(' '),
    },
    {
      id: 'chars',
      header: T.accounts.alts.characters,
      align: 'right',
      cell: (a) => a.characters,
      text: (a) => a.characters,
    },
    {
      id: 'login',
      header: T.accounts.alts.lastLogin,
      cell: (a) => formatDateTime(a.last_login_at),
      text: (a) => a.last_login_at ?? 0,
    },
    {
      id: 'banned',
      header: '',
      cell: (a) => (a.banned ? <Badge tone="danger">{T.accounts.state.banned}</Badge> : null),
    },
  ];
  return (
    <div className="plr-section">
      <p className="adm-muted">{T.accounts.alts.days(days)}</p>
      <Table
        label="Alts"
        columns={columns}
        rows={alts}
        rowKey={(a) => String(a.account_id)}
        emptyTitle={T.accounts.alts.none}
        onRowActivate={(a) => {
          navigate({ category: 'players', tab: 'accounts', params: { a: a.name } });
        }}
      />
    </div>
  );
}

function AccountBanks({ account }: { readonly account: AccountView }) {
  const characters = account.characters.filter((c) => c.status !== 'deleted');
  const [pick, setPick] = useState<number>(
    () => characters.find((c) => c.online)?.id ?? characters[0]?.id ?? 0,
  );
  if (characters.length === 0) return <p className="adm-muted">{T.accounts.banks.none}</p>;
  return (
    <div className="plr-section" data-testid="account-banks">
      <Field label={T.accounts.banks.pick}>
        <Select
          value={pick}
          onChange={(e) => {
            setPick(Number(e.target.value));
          }}
        >
          {characters.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
              {c.online ? ' (online)' : ''}
            </option>
          ))}
        </Select>
      </Field>
      <BanksOf key={pick} id={pick} name={characters.find((c) => c.id === pick)?.name ?? ''} />
    </div>
  );
}

function BanksOf({ id, name }: { readonly id: number; readonly name: string }) {
  const live = useCharacterLive(id, true);
  const value = useMemo(() => ({ id, name, live }), [id, name, live]);
  return (
    <CharacterProvider value={value}>
      <BanksInner />
    </CharacterProvider>
  );
}

function BanksInner() {
  const ui = useItemsUi();
  const viewer = useViewer();
  const equipment = useEquipment();
  return (
    <TipProvider viewer={viewer} equipment={equipment}>
      <BanksPanel ui={ui} />
    </TipProvider>
  );
}

export function AccountDrawer({ record, controls }: DrawerBodyProps) {
  const name = record.id;
  const now = Math.floor(useNow(1000) / 1000);
  const [account, setAccount] = useState<AccountView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  const [tab, setTab] = useState<TabId>('characters');
  const can = {
    ban: useCan('act.ban'),
    kick: useCan('act.kick'),
    accounts: useCan('act.accounts'),
    inventory: useCan('view.inventory'),
    pii: useCan('view.pii'),
  };
  const reload = useCallback(() => {
    setRound((n) => n + 1);
  }, []);
  useEffect(() => {
    let live = true;
    fetchAccount(name).then(
      (a) => {
        if (live) {
          setAccount(a);
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
  }, [name, round]);
  // refresh now and then: bans, sessions and the characters change under an open drawer
  useEffect(() => {
    const id = setInterval(reload, 15_000);
    return () => {
      clearInterval(id);
    };
  }, [reload]);
  useEffect(() => {
    controls.setHeader(
      account?.name ?? name,
      account ? authorityName(account.authority) : undefined,
    );
    controls.setWide(false);
  }, [controls, account, name]);
  if (error && !account) return <ErrorState error={error} onRetry={reload} />;
  if (!account) return <Skeleton lines={8} height={18} />;

  const tabs: TabItem[] = [
    { id: 'characters', label: T.accounts.tabs.characters, badge: account.characters.length },
    ...(can.inventory ? [{ id: 'banks', label: T.accounts.tabs.banks }] : []),
    { id: 'bans', label: T.accounts.tabs.bans, badge: account.bans.length },
    { id: 'logins', label: T.accounts.tabs.logins },
    ...(can.pii ? [{ id: 'alts', label: T.accounts.tabs.alts }] : []),
    { id: 'notes', label: T.accounts.tabs.notes, badge: account.notes.length },
  ];
  const active: TabId = tabs.some((t) => t.id === tab) ? tab : 'characters';
  const staff = 'LMNOP'.includes(account.effective_authority);
  const items: MenuItem[] = [];
  if (can.accounts) {
    items.push(
      {
        id: 'reset',
        label: T.accounts.actions.resetPassword,
        onSelect: () => void resetAccountPassword(account.name),
      },
      {
        id: 'auth',
        label: T.accounts.actions.setAuthority,
        onSelect: () =>
          void changeAuthority(account.name, account.authority).then((ok) => {
            if (ok) reload();
          }),
      },
    );
    if (staff)
      items.push({
        id: 'end',
        label: T.accounts.actions.endSessions,
        onSelect: () => void endSessionsOf(account.name),
      });
  }
  items.push({
    id: 'copy',
    label: T.actions.copyLink,
    onSelect: () => {
      controls.copyLink();
    },
  });
  const pinnedNotes = account.notes.filter((n) => n.pinned);
  return (
    <div className="plr-drawer" data-testid="account-drawer" data-account={account.name}>
      <div className="plr-head">
        <div className="plr-head__who">
          <div className="plr-head__text">
            <strong className="plr-head__name">{account.name}</strong>
            <span className="adm-muted">
              {T.accounts.header.created} {formatDateTime(account.created_at)} (
              {account.created_via})
            </span>
          </div>
          <span className="plr-head__badges">
            {account.online ? (
              <Badge tone="ok">{T.common.online}</Badge>
            ) : (
              <Badge>{T.common.offline}</Badge>
            )}
            {account.banned ? (
              <Badge tone="danger">{T.accounts.state.banned}</Badge>
            ) : (
              <Badge tone="ok">{T.accounts.state.active}</Badge>
            )}
            {account.must_change_password ? (
              <Badge tone="warn">{T.accounts.state.mustChange}</Badge>
            ) : null}
            {account.totp_enabled ? <Badge>{T.accounts.state.totp}</Badge> : null}
          </span>
        </div>
        <div className="plr-actions" role="toolbar" aria-label="Account actions">
          {can.ban ? (
            account.banned ? (
              <Button
                size="sm"
                onClick={() =>
                  void unban(account.name).then((ok) => {
                    if (ok) reload();
                  })
                }
              >
                {T.accounts.actions.unban}
              </Button>
            ) : (
              <Button
                size="sm"
                variant="danger"
                onClick={() =>
                  void banWholeAccount(account.name).then((ok) => {
                    if (ok) reload();
                  })
                }
              >
                {T.accounts.actions.ban}
              </Button>
            )
          ) : null}
          {can.kick ? (
            <Button size="sm" variant="danger" onClick={() => void kickAccount(account.name)}>
              {T.accounts.actions.kickAll}
            </Button>
          ) : null}
          <Menu label={T.common.more} trigger={<span>{T.common.more}</span>} items={items} />
        </div>
      </div>
      <dl className="adm-dl plr-dl">
        <dt>{T.accounts.header.authority}</dt>
        <dd>
          <Badge tone="accent">{authorityName(account.authority)}</Badge>
          {account.effective_authority !== account.authority ? (
            <span className="adm-muted">
              {' '}
              {T.accounts.header.effective}: {authorityName(account.effective_authority)}
            </span>
          ) : null}
        </dd>
        <dt>{T.accounts.header.lastLogin}</dt>
        <dd>
          {formatDateTime(account.last_login_at)}
          {account.last_login_ip ? <code> {account.last_login_ip}</code> : null}
        </dd>
        {account.registration_ip ? (
          <>
            <dt>{T.accounts.header.regIp}</dt>
            <dd>
              <code>{account.registration_ip}</code>
            </dd>
          </>
        ) : null}
        {account.email ? (
          <>
            <dt>{T.accounts.header.email}</dt>
            <dd>{account.email}</dd>
          </>
        ) : null}
        {account.banned ? (
          <>
            <dt>{T.accounts.tabs.bans}</dt>
            <dd>
              {account.banned.until === null
                ? T.accounts.bans.permanent
                : `${T.accounts.bans.until} ${formatDateTime(account.banned.until)} (${formatSpan(account.banned.until - now)})`}
              {`: ${account.banned.reason}`}
            </dd>
          </>
        ) : null}
      </dl>
      {pinnedNotes.length > 0 ? (
        <div className="adm-alert adm-alert--warn" role="note" aria-label={T.drawer.pinnedNotes}>
          <NoteList notes={pinnedNotes} onChanged={reload} />
        </div>
      ) : null}
      <Tabs
        tabs={tabs}
        value={active}
        onChange={(t) => {
          setTab(t as TabId);
        }}
        idPrefix="plr-acct"
        label="Account"
      />
      <div
        role="tabpanel"
        id={tabPanelId('plr-acct')}
        aria-labelledby={tabButtonId('plr-acct', active)}
        className="plr-panel"
      >
        {active === 'characters' ? <AccountCharacters characters={account.characters} /> : null}
        {active === 'banks' ? <AccountBanks account={account} /> : null}
        {active === 'bans' ? <BansTable bans={account.bans} /> : null}
        {active === 'logins' ? (
          <div className="plr-section">
            <p className="adm-muted">{T.accounts.logins.hint}</p>
            <AuditTimeline
              subject={{ account: account.id }}
              groups={['account']}
              filterable={false}
              empty={T.history.emptyLogins}
            />
          </div>
        ) : null}
        {active === 'alts' ? <AltsPanel name={account.name} /> : null}
        {active === 'notes' ? (
          <div className="plr-section">
            <NoteList notes={account.notes} onChanged={reload} />
            <AddNote kind="account" subjectId={account.id} onAdded={reload} />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function AccountCharacters({ characters }: { readonly characters: readonly CharacterListItem[] }) {
  const columns: Column<CharacterListItem>[] = [
    {
      id: 'slot',
      header: 'Slot',
      cell: (c) => T.accounts.characters.slot(c.slot),
      text: (c) => c.slot,
      width: '70px',
    },
    {
      id: 'name',
      header: T.characters.columns.name,
      cell: (c) => (
        <span className="adm-row">
          <JobIcon job={c.job} size={20} />
          <strong>{c.name}</strong>
        </span>
      ),
      text: (c) => c.name,
    },
    {
      id: 'level',
      header: T.characters.columns.level,
      align: 'right',
      cell: (c) => c.level,
      text: (c) => c.level,
    },
    {
      id: 'job',
      header: T.characters.columns.job,
      cell: (c) => jobName(c.job),
      text: (c) => jobName(c.job),
    },
    {
      id: 'state',
      header: T.characters.columns.state,
      cell: (c) => <StatusChips c={c} />,
      text: (c) => c.status,
    },
  ];
  return (
    <Table
      label="Characters of the account"
      columns={columns}
      rows={characters}
      rowKey={(c) => String(c.id)}
      emptyTitle={T.accounts.characters.none}
      onRowActivate={(c) => {
        openCharacter(c.id);
      }}
    />
  );
}
