// Players -> Moderation (Plan/25 section 4.8): the sub-tabs Bans, Mutes, Login blocks, IP blocks, Notes, Alts and Chat. Each list is one
// of the moderation routes of the players backend; the actions are the same dialogs as everywhere (reason, duration chips).
import { useCallback, useEffect, useState } from 'react';
import { rest } from '../../api/rest';
import { runConfirmed } from '../../app/mutate';
import { DURATION_CHIPS } from '../../app/confirm';
import { withParams } from '../../app/route';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Button,
  Countdown,
  ErrorState,
  Field,
  Skeleton,
  Table,
  Tabs,
  TextInput,
  tabButtonId,
  tabPanelId,
  usePagedList,
  type Column,
  type TabItem,
} from '../../components';
import type {
  AltClusterView,
  BanView,
  CharacterListItem,
  IpBlocksView,
  MuteView,
  NoteView,
} from '../../gen/dto';
import { navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import { unban } from './accountActions';
import { ChatMonitor } from './ChatMonitor';
import { addIpBlock, fetchAltClusters, fetchIpBlocks, removeIpBlock, unmuteCharacter } from './api';
import { liftLoginBlock } from './characterActions';
import { formatDateTime } from './format';
import { openAccount } from './AccountsTab';
import { NoteList } from './NotesPanel';
import { openCharacter } from './rowMenu';
import { T } from './strings';

type Sub = 'bans' | 'mutes' | 'blocks' | 'ip' | 'notes' | 'alts' | 'chat';
const SUBS: readonly Sub[] = ['bans', 'mutes', 'blocks', 'ip', 'notes', 'alts', 'chat'];

const isSub = (v: string | undefined): v is Sub =>
  v !== undefined && (SUBS as readonly string[]).includes(v);

export function ModerationTab({ route }: TabProps) {
  const canPii = useCan('view.pii');
  const canChat = useCan('view.chat');
  const requested = route.params['ms'];
  const [sub, setSubState] = useState<Sub>(isSub(requested) ? requested : 'bans');
  const setSub = (s: Sub): void => {
    setSubState(s);
    navigate(withParams(route, { ms: s === 'bans' ? undefined : s }), { replace: true });
  };
  const tabs: TabItem[] = [
    { id: 'bans', label: T.moderation.tabs.bans },
    { id: 'mutes', label: T.moderation.tabs.mutes },
    { id: 'blocks', label: T.moderation.tabs.blocks },
    ...(canPii ? [{ id: 'ip', label: T.moderation.tabs.ip }] : []),
    { id: 'notes', label: T.moderation.tabs.notes },
    ...(canPii ? [{ id: 'alts', label: T.moderation.tabs.alts }] : []),
    ...(canChat ? [{ id: 'chat', label: T.moderation.tabs.chat }] : []),
  ];
  const active: Sub = tabs.some((t) => t.id === sub) ? sub : 'bans';
  return (
    <div className="plr-tab" data-testid="moderation-tab">
      <Tabs
        tabs={tabs}
        value={active}
        onChange={(t) => {
          setSub(t as Sub);
        }}
        idPrefix="plr-mod"
        label="Moderation"
      />
      <div
        role="tabpanel"
        id={tabPanelId('plr-mod')}
        aria-labelledby={tabButtonId('plr-mod', active)}
      >
        {active === 'bans' ? <Bans /> : null}
        {active === 'mutes' ? <Mutes /> : null}
        {active === 'blocks' ? <Blocks /> : null}
        {active === 'ip' ? <IpBlocks /> : null}
        {active === 'notes' ? <AllNotes /> : null}
        {active === 'alts' ? <AltClusters /> : null}
        {active === 'chat' ? <ChatMonitor /> : null}
      </div>
    </div>
  );
}

function Bans() {
  const canBan = useCan('act.ban');
  const list = usePagedList<BanView>({ path: '/admin/api/moderation/bans', limit: 100 });
  const columns: Column<BanView>[] = [
    {
      id: 'account',
      header: T.moderation.bans.account,
      cell: (b) => (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            openAccount(b.account);
          }}
        >
          {b.account}
        </Button>
      ),
      text: (b) => b.account,
      sortable: true,
    },
    {
      id: 'reason',
      header: T.moderation.bans.reason,
      cell: (b) => b.reason,
      text: (b) => b.reason,
    },
    { id: 'by', header: T.moderation.bans.by, cell: (b) => b.by, text: (b) => b.by },
    {
      id: 'since',
      header: T.moderation.bans.since,
      cell: (b) => formatDateTime(b.created_at),
      text: (b) => b.created_at,
      sortable: true,
    },
    {
      id: 'ends',
      header: T.moderation.bans.ends,
      cell: (b) =>
        b.until === null ? (
          <Badge tone="danger">{T.accounts.bans.permanent}</Badge>
        ) : (
          <Countdown until={b.until} />
        ),
      text: (b) => b.until ?? Number.MAX_SAFE_INTEGER,
      sortable: true,
    },
    {
      id: 'unban',
      header: '',
      cell: (b) =>
        canBan ? (
          <Button
            size="sm"
            onClick={() =>
              void unban(b.account).then((ok) => {
                if (ok) list.reload();
              })
            }
          >
            {T.moderation.bans.unban}
          </Button>
        ) : null,
    },
  ];
  return (
    <Table
      label="Active bans"
      columns={columns}
      rows={list.rows}
      rowKey={(b) => String(b.id)}
      loading={list.loading}
      error={list.error}
      onRetry={list.reload}
      hasMore={list.hasMore}
      loadingMore={list.loadingMore}
      onLoadMore={list.loadMore}
      emptyTitle={T.moderation.bans.none}
      exportName="active-bans"
    />
  );
}

function Mutes() {
  const canMute = useCan('act.mute');
  const [rows, setRows] = useState<readonly MuteView[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  useEffect(() => {
    let live = true;
    rest.get<{ items: MuteView[] }>('/admin/api/moderation/mutes').then(
      (p) => {
        if (live) {
          setRows(p.items);
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
  }, [round]);
  useEffect(() => {
    const id = setInterval(() => {
      setRound((n) => n + 1);
    }, 10_000);
    return () => {
      clearInterval(id);
    };
  }, []);
  if (error && !rows)
    return (
      <ErrorState
        error={error}
        onRetry={() => {
          setRound((n) => n + 1);
        }}
      />
    );
  if (!rows) return <Skeleton lines={4} height={18} />;
  const columns: Column<MuteView>[] = [
    {
      id: 'name',
      header: T.moderation.mutes.character,
      cell: (m) => (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            openCharacter(m.character_id);
          }}
        >
          {m.name}
        </Button>
      ),
      text: (m) => m.name,
      sortable: true,
    },
    {
      id: 'left',
      header: T.moderation.mutes.left,
      cell: (m) => <Countdown until={m.until} />,
      text: (m) => m.remaining_s,
      sortable: true,
    },
    {
      id: 'unmute',
      header: '',
      cell: (m) =>
        canMute ? (
          <Button
            size="sm"
            onClick={() => {
              void runConfirmed({
                title: T.actions.unmute,
                message: m.name,
                confirmLabel: T.actions.unmute,
                run: (given) => unmuteCharacter(m.character_id, given),
                success: T.dialogs.unmuted(m.name),
              }).then((o) => {
                if (o.ok) setRound((n) => n + 1);
              });
            }}
          >
            {T.moderation.mutes.unmute}
          </Button>
        ) : null,
    },
  ];
  return (
    <div className="plr-section">
      <p className="adm-muted adm-small">{T.moderation.mutes.note}</p>
      <Table
        label="Active mutes"
        columns={columns}
        rows={rows}
        rowKey={(m) => String(m.character_id)}
        emptyTitle={T.moderation.mutes.none}
        exportName="active-mutes"
      />
    </div>
  );
}

function Blocks() {
  const canBan = useCan('act.ban');
  const list = usePagedList<CharacterListItem>({
    path: '/admin/api/moderation/blocks',
    limit: 100,
  });
  const columns: Column<CharacterListItem>[] = [
    {
      id: 'name',
      header: T.moderation.blocks.character,
      cell: (c) => (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            openCharacter(c.id);
          }}
        >
          {c.name}
        </Button>
      ),
      text: (c) => c.name,
      sortable: true,
    },
    {
      id: 'account',
      header: T.characters.columns.account,
      cell: (c) => c.account,
      text: (c) => c.account,
    },
    {
      id: 'until',
      header: T.moderation.blocks.until,
      cell: (c) =>
        c.login_blocked_until === null
          ? '-'
          : c.login_blocked_until > 4_000_000_000
            ? T.moderation.blocks.forever
            : formatDateTime(c.login_blocked_until),
      text: (c) => c.login_blocked_until ?? 0,
      sortable: true,
    },
    {
      id: 'lift',
      header: '',
      cell: (c) =>
        canBan ? (
          <Button
            size="sm"
            onClick={() =>
              void liftLoginBlock({ id: c.id, name: c.name }).then((ok) => {
                if (ok) list.reload();
              })
            }
          >
            {T.moderation.blocks.lift}
          </Button>
        ) : null,
    },
  ];
  return (
    <Table
      label="Login blocks"
      columns={columns}
      rows={list.rows}
      rowKey={(c) => String(c.id)}
      loading={list.loading}
      error={list.error}
      onRetry={list.reload}
      hasMore={list.hasMore}
      loadingMore={list.loadingMore}
      onLoadMore={list.loadMore}
      emptyTitle={T.moderation.blocks.none}
      exportName="login-blocks"
    />
  );
}

function AddIp({ onAdded }: { readonly onAdded: () => void }) {
  const cidr = { current: '' };
  return (
    <Button
      onClick={() => {
        void runConfirmed({
          title: T.moderation.ip.addTitle,
          message: (
            <CidrField
              onValue={(text) => {
                cidr.current = text;
              }}
            />
          ),
          confirmLabel: T.moderation.ip.block,
          danger: true,
          reason: 'required',
          durations: DURATION_CHIPS,
          run: (given) => {
            const text = cidr.current.trim();
            if (text === '') throw new Error(T.moderation.ip.invalidCidr);
            const secs = given.duration?.seconds;
            return addIpBlock(
              text,
              secs === null || secs === undefined ? null : Math.floor(Date.now() / 1000) + secs,
              given,
            ).then(() => text);
          },
          success: (c) => T.moderation.ip.added(c),
        }).then((o) => {
          if (o.ok) onAdded();
        });
      }}
    >
      {T.moderation.ip.add}
    </Button>
  );
}

function CidrField({ onValue }: { readonly onValue: (text: string) => void }) {
  const [text, setText] = useState('');
  return (
    <Field label={T.moderation.ip.cidrField} hint={T.moderation.ip.cidrHint}>
      <TextInput
        data-autofocus
        value={text}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => {
          setText(e.target.value);
          onValue(e.target.value);
        }}
      />
    </Field>
  );
}

function IpBlocks() {
  const canBlock = useCan('act.ip_block');
  const [data, setData] = useState<IpBlocksView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  const reload = useCallback(() => {
    setRound((n) => n + 1);
  }, []);
  useEffect(() => {
    let live = true;
    fetchIpBlocks().then(
      (d) => {
        if (live) {
          setData(d);
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
  }, [round]);
  if (error && !data) return <ErrorState error={error} onRetry={reload} />;
  if (!data) return <Skeleton lines={4} height={18} />;
  const columns: Column<IpBlocksView['table'][number]>[] = [
    {
      id: 'cidr',
      header: T.moderation.ip.cidr,
      cell: (b) => <code>{b.cidr}</code>,
      text: (b) => b.cidr,
    },
    { id: 'reason', header: T.moderation.ip.reason, cell: (b) => b.reason, text: (b) => b.reason },
    {
      id: 'until',
      header: T.moderation.ip.until,
      cell: (b) =>
        b.until === null ? (
          T.moderation.blocks.forever
        ) : b.active ? (
          <Countdown until={b.until} />
        ) : (
          <Badge>{T.moderation.ip.expired}</Badge>
        ),
      text: (b) => b.until ?? Number.MAX_SAFE_INTEGER,
    },
    { id: 'by', header: T.moderation.ip.by, cell: (b) => b.created_by, text: (b) => b.created_by },
    {
      id: 'remove',
      header: '',
      cell: (b) =>
        canBlock ? (
          <Button
            size="sm"
            onClick={() => {
              void runConfirmed({
                title: T.moderation.ip.removeTitle(b.cidr),
                confirmLabel: T.moderation.ip.remove,
                danger: true,
                run: () => removeIpBlock(b.id),
                success: T.moderation.ip.removed(b.cidr),
              }).then((o) => {
                if (o.ok) reload();
              });
            }}
          >
            {T.moderation.ip.remove}
          </Button>
        ) : null,
    },
  ];
  return (
    <div className="plr-section">
      {canBlock ? (
        <div>
          <AddIp onAdded={reload} />
        </div>
      ) : null}
      <h3>{T.moderation.ip.tableTitle}</h3>
      <Table
        label="IP blocks"
        columns={columns}
        rows={data.table}
        rowKey={(b) => String(b.id)}
        emptyTitle={T.moderation.ip.none}
      />
      <h3>{T.moderation.ip.configTitle}</h3>
      {data.config.length === 0 ? (
        <p className="adm-muted">{T.moderation.ip.none}</p>
      ) : (
        <ul className="plr-list">
          {data.config.map((c) => (
            <li key={c}>
              <code>{c}</code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function AllNotes() {
  const list = usePagedList<NoteView>({ path: '/admin/api/notes', limit: 50 });
  if (list.error && list.rows.length === 0)
    return <ErrorState error={list.error} onRetry={list.reload} />;
  if (list.loading) return <Skeleton lines={4} height={18} />;
  return (
    <div className="plr-section">
      <p className="adm-muted adm-small">{T.moderation.notes.all}</p>
      <NoteList notes={list.rows} onChanged={list.reload} showSubject />
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

function AltClusters() {
  const [days, setDays] = useState(30);
  const [clusters, setClusters] = useState<readonly AltClusterView[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let live = true;
    fetchAltClusters().then(
      (d) => {
        if (live) {
          setClusters(d.clusters);
          setDays(d.days);
        }
      },
      (e: unknown) => {
        if (live) setError(e);
      },
    );
    return () => {
      live = false;
    };
  }, []);
  if (error) return <ErrorState error={error} />;
  if (!clusters) return <Skeleton lines={4} height={18} />;
  const columns: Column<AltClusterView>[] = [
    {
      id: 'ip',
      header: T.moderation.alts.address,
      cell: (c) => <code>{c.ip}</code>,
      text: (c) => c.ip,
    },
    {
      id: 'accounts',
      header: T.moderation.alts.accounts,
      cell: (c) => (
        <span className="adm-row">
          {c.accounts.map((a) => (
            <Button
              key={a.account_id}
              size="sm"
              variant={a.banned ? 'danger' : 'secondary'}
              onClick={() => {
                openAccount(a.name);
              }}
            >
              {a.name}
            </Button>
          ))}
          {c.total > c.accounts.length ? (
            <span className="adm-muted">{T.moderation.alts.more(c.total - c.accounts.length)}</span>
          ) : null}
        </span>
      ),
      text: (c) => c.accounts.map((a) => a.name).join(' '),
    },
    {
      id: 'total',
      header: T.moderation.alts.total,
      align: 'right',
      cell: (c) => c.total,
      text: (c) => c.total,
      sortable: true,
    },
  ];
  return (
    <div className="plr-section">
      <p className="adm-muted">{T.accounts.alts.days(days)}</p>
      <Table
        label="Alt clusters"
        columns={columns}
        rows={clusters}
        rowKey={(c) => c.ip}
        emptyTitle={T.moderation.alts.none}
        exportName="alt-clusters"
      />
    </div>
  );
}
