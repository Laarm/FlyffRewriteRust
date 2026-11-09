// The panels of the guild drawer (Plan/25 section 6.1): the roster, the bank, the bank log, the wars, the House placeholder and the history.
import { useEffect, useMemo, useState } from 'react';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { fetchPage } from '../../api/rest';
import { navigate } from '../../stores/route';
import {
  Badge,
  Button,
  Field,
  Menu,
  NeedsFeature,
  Select,
  Table,
  usePagedList,
  type Column,
  type MenuItem,
} from '../../components';
import type {
  GuildDetailView,
  GuildLogRow,
  GuildMemberView,
  GuildRank,
  GuildWarRow,
} from '../../gen/dto';
import { useCan } from '../../stores/session';
import { useNow } from '../../app/polling';
import { dataOf, num, type AuditRow } from '../audit/api';
import { decodeFrame, type ContainerModel } from '../players/containers';
import { formatAgo, formatDateTime, formatNumber } from '../players/format';
import { ItemGrid, type GridCommon } from '../players/Grids';
import { usePlayerGame } from '../players/gamedata';
import { JobIcon } from '../players/JobIcon';
import { NO_FILTER } from '../players/containers';
import { openCharacter } from '../players/rowMenu';
import { TipProvider } from '../players/Tip';
import { NO_EQUIPMENT } from '../players/tooltip';
import { applyEdit, askForm, changeRank, makeMaster, removeMember, type GuildRef } from './actions';
import { G, RANK_ORDER } from './strings';
import { openGuild } from './GuildsTab';

// ---- roster -----------------------------------------------------------------------------------------------------------------------

export function RosterPanel({
  detail,
  guild,
  canAct,
}: {
  readonly detail: GuildDetailView;
  readonly guild: GuildRef;
  readonly canAct: boolean;
}) {
  const now = Math.floor(useNow(30_000) / 1000);
  const columns: Column<GuildMemberView>[] = [
    {
      id: 'rank',
      header: G.roster.columns.rank,
      cell: (m) => G.rank[m.rank],
      text: (m) => G.rank[m.rank],
    },
    {
      id: 'name',
      header: G.roster.columns.name,
      cell: (m) => (
        <span className="adm-row">
          <JobIcon job={m.job} size={20} />
          <strong>{m.name}</strong>
        </span>
      ),
      text: (m) => m.name,
    },
    {
      id: 'level',
      header: G.roster.columns.level,
      align: 'right',
      cell: (m) => m.level,
      text: (m) => m.level,
    },
    {
      id: 'online',
      header: G.roster.columns.online,
      cell: (m) => (
        <span
          className={m.online ? 'adm-dot is-online' : 'adm-dot'}
          title={m.online ? 'online' : 'offline'}
        >
          {m.online ? '●' : '○'}
        </span>
      ),
      text: (m) => (m.online ? 'online' : 'offline'),
    },
    {
      id: 'gold',
      header: G.roster.columns.gold,
      align: 'right',
      cell: (m) => formatNumber(m.give_gold),
      text: (m) => m.give_gold,
    },
    {
      id: 'pxp',
      header: G.roster.columns.pxp,
      align: 'right',
      cell: (m) => formatNumber(m.give_pxp),
      text: (m) => m.give_pxp,
    },
    { id: 'alias', header: G.roster.columns.alias, cell: (m) => m.alias, text: (m) => m.alias },
    {
      id: 'joined',
      header: G.roster.columns.joined,
      cell: (m) => formatDateTime(m.joined_at),
      text: (m) => m.joined_at,
    },
    {
      id: 'seen',
      header: G.roster.columns.seen,
      cell: (m) => (m.online ? 'now' : formatAgo(now, m.last_seen)),
      text: (m) => m.last_seen ?? 0,
    },
    {
      id: 'menu',
      header: '',
      width: '48px',
      cell: (m) => <MemberMenu detail={detail} guild={guild} member={m} canAct={canAct} />,
    },
  ];
  return (
    <div className="plr-section" data-testid="guild-roster">
      <ul className="gld-ranks" aria-label="Places per rank">
        {RANK_ORDER.map((r, i) => (
          <li key={r}>
            <Badge>{`${G.rank[r]} ${G.roster.cap(detail.members.filter((m) => m.rank === r).length, detail.rank_caps[i] ?? 0)}`}</Badge>
          </li>
        ))}
      </ul>
      <Table
        label="Roster"
        columns={columns}
        rows={detail.members}
        rowKey={(m) => String(m.character)}
        emptyTitle={G.roster.empty}
        exportName="guild-roster"
      />
    </div>
  );
}

function MemberMenu({
  detail,
  guild,
  member,
  canAct,
}: {
  readonly detail: GuildDetailView;
  readonly guild: GuildRef;
  readonly member: GuildMemberView;
  readonly canAct: boolean;
}) {
  const items: MenuItem[] = [
    {
      id: 'open',
      label: G.roster.menu.open,
      onSelect: () => {
        openCharacter(member.character);
      },
    },
  ];
  if (canAct) {
    if (member.rank !== 'master') {
      items.push(
        {
          id: 'rank',
          label: G.roster.menu.rank,
          onSelect: () => {
            void changeRank(guild, detail, member);
          },
        },
        {
          id: 'master',
          label: G.roster.menu.master,
          onSelect: () => {
            void makeMaster(guild, member);
          },
        },
      );
    }
    items.push({
      id: 'remove',
      label: G.roster.menu.remove,
      onSelect: () => {
        void removeMember(guild, member);
      },
    });
  }
  return <Menu label={member.name} trigger="..." items={items} />;
}

// ---- bank -------------------------------------------------------------------------------------------------------------------------

/** The grid model of the guild bank (the frame of the REST view; the kind is the one of a bank grid). */
export function bankModel(detail: GuildDetailView): ContainerModel | null {
  const snap = decodeFrame(detail.bank.pb);
  if (!snap) return null;
  const cells = new Map<number, ItemElem>();
  for (const item of snap.items) cells.set(item.objId, item);
  return {
    key: `guild-bank:${String(detail.guild.id)}`,
    kind: 'bank',
    index: 0,
    owner: detail.guild.id,
    ownerName: detail.guild.name,
    rev: detail.bank.rev,
    capacity: detail.bank.capacity,
    gold: detail.bank.gold,
    live: true,
    savedAt: 0,
    liveVia: 0,
    liveViaName: '',
    cells,
  };
}

export function BankPanel({
  detail,
  guild,
  flash,
  canInventory,
}: {
  readonly detail: GuildDetailView;
  readonly guild: GuildRef;
  readonly flash: ReadonlyMap<number, number>;
  readonly canInventory: boolean;
}) {
  const game = usePlayerGame();
  const canItems = useCan('act.items');
  const [selected, setSelected] = useState<number | null>(null);
  const model = useMemo(() => bankModel(detail), [detail]);
  if (detail.bank_hidden || !canInventory) {
    return (
      <div className="plr-section" data-testid="guild-bank">
        <p className="adm-alert adm-alert--warn">{G.bank.needsInventory}</p>
        <p>{G.bank.pool(detail.bank.gold)}</p>
      </div>
    );
  }
  const common: GridCommon = {
    game: game.data,
    filter: NO_FILTER,
    flash,
    selected,
    onSelect: setSelected,
    onOpen: setSelected,
  };
  const elem = selected === null ? undefined : model?.cells.get(selected);
  const name = elem ? (game.data?.byId.get(elem.itemId)?.name ?? `#${String(elem.itemId)}`) : '';
  return (
    <TipProvider viewer={{ sex: 0, job: 0, level: 1 }} equipment={NO_EQUIPMENT}>
      <div className="plr-section" data-testid="guild-bank">
        <p className="adm-muted adm-small">{G.bank.intro}</p>
        <p data-testid="bank-pool">{G.bank.pool(detail.bank.gold)}</p>
        {model ? <ItemGrid model={model} common={common} label="Guild bank" /> : null}
        {model?.cells.size === 0 ? <p className="adm-muted">{G.bank.empty}</p> : null}
        <div className="adm-row">
          <span className="adm-muted adm-small">{elem ? G.bank.selected(name) : G.bank.none}</span>
          {canItems ? (
            <Button
              disabled={!elem}
              onClick={() => {
                if (elem) void removeFromBank(guild, elem, name);
              }}
            >
              {G.bank.remove}
            </Button>
          ) : null}
        </div>
        {canItems ? <p className="adm-muted adm-small">{G.bank.hint}</p> : null}
      </div>
    </TipProvider>
  );
}

async function removeFromBank(guild: GuildRef, elem: ItemElem, name: string): Promise<void> {
  const count = await askForm<number>({
    title: G.dialogs.bankRemoveTitle(name),
    initial: elem.num,
    valid: (v) => Number.isInteger(v) && v >= 1 && v <= elem.num,
    body: (v, set) => (
      <Field label={G.dialogs.bankCount} hint={`1 - ${String(elem.num)}`}>
        <input
          className="adm-input adm-input--number"
          type="number"
          min={1}
          max={elem.num}
          value={v}
          onChange={(e) => {
            set(Math.trunc(Number(e.target.value)));
          }}
        />
      </Field>
    ),
  });
  if (count === null) return;
  await applyEdit(
    guild,
    { bank_remove: { slot: elem.objId, serial: Number(elem.serial), count } },
    {
      title: G.dialogs.bankRemoveTitle(name),
      reason: 'required',
      irreversible: elem.abilityOption >= 7,
      items: count,
    },
  );
}

// ---- bank log ---------------------------------------------------------------------------------------------------------------------

export function LogPanel({
  id,
  canInventory,
}: {
  readonly id: number;
  readonly canInventory: boolean;
}) {
  const [kind, setKind] = useState('');
  const game = usePlayerGame();
  const list = usePagedList<GuildLogRow>({
    path: `/admin/api/guilds/${String(id)}/log`,
    query: kind === '' ? {} : { kind },
    enabled: canInventory,
    limit: 100,
  });
  if (!canInventory) return <p className="adm-alert adm-alert--warn">{G.log.needsInventory}</p>;
  const itemName = (r: GuildLogRow): string =>
    r.item_id === 0
      ? ''
      : (game.data?.byId.get(r.item_id)?.name ?? `#${String(r.item_id)}`) +
        (r.upgrade_level > 0 ? ` +${String(r.upgrade_level)}` : '');
  const columns: Column<GuildLogRow>[] = [
    {
      id: 'when',
      header: G.log.columns.when,
      cell: (r) => formatDateTime(r.at),
      text: (r) => r.at,
    },
    {
      id: 'kind',
      header: G.log.columns.kind,
      cell: (r) => (G.log.kinds as Record<string, string>)[r.kind] ?? r.kind,
      text: (r) => r.kind,
    },
    {
      id: 'who',
      header: G.log.columns.who,
      cell: (r) => (r.character_id === 0 ? G.log.system : r.character_name),
      text: (r) => r.character_name,
    },
    { id: 'item', header: G.log.columns.item, cell: itemName, text: itemName },
    {
      id: 'count',
      header: G.log.columns.count,
      align: 'right',
      cell: (r) => formatNumber(r.count),
      text: (r) => r.count,
    },
  ];
  return (
    <div className="plr-section" data-testid="guild-log">
      <Field label={G.log.kind}>
        <Select
          value={kind}
          onChange={(e) => {
            setKind(e.target.value);
          }}
        >
          <option value="">{G.log.all}</option>
          {Object.entries(G.log.kinds).map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </Select>
      </Field>
      <Table
        label="Bank log"
        columns={columns}
        rows={list.rows}
        rowKey={(r) => String(r.id)}
        loading={list.loading}
        error={list.error}
        onRetry={list.reload}
        hasMore={list.hasMore}
        loadingMore={list.loadingMore}
        onLoadMore={list.loadMore}
        emptyTitle={G.log.empty}
        exportName="guild-bank-log"
      />
    </div>
  );
}

// ---- wars -------------------------------------------------------------------------------------------------------------------------

export function WarsPanel({ detail }: { readonly detail: GuildDetailView }) {
  const list = usePagedList<GuildWarRow>({
    path: `/admin/api/guilds/${String(detail.guild.id)}/wars`,
    limit: 50,
  });
  const columns: Column<GuildWarRow>[] = [
    {
      id: 'started',
      header: G.wars.columns.started,
      cell: (w) => formatDateTime(w.started_at),
      text: (w) => w.started_at,
    },
    {
      id: 'ended',
      header: G.wars.columns.ended,
      cell: (w) => (w.ended_at === null ? G.wars.running : formatDateTime(w.ended_at)),
      text: (w) => w.ended_at ?? 0,
    },
    {
      id: 'sides',
      header: G.wars.columns.sides,
      cell: (w) => `${w.declarer_name} vs ${w.acceptor_name}`,
      text: (w) => `${w.declarer_name} vs ${w.acceptor_name}`,
    },
    {
      id: 'outcome',
      header: G.wars.columns.outcome,
      cell: (w) =>
        w.outcome === null
          ? G.wars.running
          : ((G.wars.outcome as Record<string, string>)[w.outcome] ?? w.outcome),
      text: (w) => w.outcome ?? '',
    },
    {
      id: 'deaths',
      header: G.wars.columns.deaths,
      cell: (w) => `${String(w.decl.deaths)} / ${String(w.acpt.deaths)}`,
      text: (w) => w.decl.deaths + w.acpt.deaths,
    },
    {
      id: 'points',
      header: G.wars.columns.points,
      cell: (w) =>
        `${w.decl_points_after === null ? '-' : String(w.decl_points_after)} / ${w.acpt_points_after === null ? '-' : String(w.acpt_points_after)}`,
      text: (w) => w.decl_points_after ?? 0,
    },
  ];
  return (
    <div className="plr-section" data-testid="guild-wars">
      <p className="adm-row">
        <strong>{G.wars.current}:</strong>{' '}
        {detail.war ? (
          <button
            type="button"
            className="adm-linkbtn"
            onClick={() => {
              openGuild(detail.war?.enemy_id ?? 0);
            }}
          >
            {detail.war.enemy_name === ''
              ? `#${String(detail.war.enemy_id)}`
              : detail.war.enemy_name}
          </button>
        ) : (
          G.wars.none
        )}
      </p>
      <p className="adm-muted adm-small">
        {G.drawer.warRecord(
          detail.war_record.wins,
          detail.war_record.losses,
          detail.war_record.surrenders,
          detail.war_record.points,
        )}
      </p>
      <Table
        label="Wars"
        columns={columns}
        rows={list.rows}
        rowKey={(w) => String(w.id)}
        loading={list.loading}
        error={list.error}
        onRetry={list.reload}
        hasMore={list.hasMore}
        loadingMore={list.loadingMore}
        onLoadMore={list.loadMore}
        emptyTitle={G.wars.none}
      />
    </div>
  );
}

// ---- house + history --------------------------------------------------------------------------------------------------------------

export function HousePanel() {
  return <NeedsFeature wp={G.drawer.houseWp} />;
}

export function HistoryPanel({ id }: { readonly id: number }) {
  const canAudit = useCan('view.audit');
  const canOwn = useCan('view.audit.own');
  const [rows, setRows] = useState<readonly AuditRow[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    if (!canAudit && !canOwn) return;
    let live = true;
    const from = Date.now() - 30 * 86_400_000;
    fetchPage<AuditRow>('/admin/api/audit', {
      groups: 'guild',
      events: 'admin.guild',
      q: `"guild":${String(id)}`,
      from,
      limit: 200,
    }).then(
      (page) => {
        if (live) setRows(page.items.filter((r) => num(dataOf(r)['guild']) === id));
      },
      (e: unknown) => {
        if (live) setError(e);
      },
    );
    return () => {
      live = false;
    };
  }, [id, canAudit, canOwn]);
  if (!canAudit && !canOwn)
    return <p className="adm-alert adm-alert--warn">{G.history.needsAudit}</p>;
  const columns: Column<AuditRow>[] = [
    {
      id: 'when',
      header: G.log.columns.when,
      cell: (r) => formatDateTime(Math.floor(r.ts / 1000)),
      text: (r) => r.ts,
    },
    { id: 'ev', header: 'Event', cell: (r) => r.ev, text: (r) => r.ev },
    { id: 'actor', header: 'Actor', cell: (r) => r.actor ?? '', text: (r) => r.actor ?? '' },
    { id: 'summary', header: 'What', cell: (r) => r.summary, text: (r) => r.summary },
  ];
  return (
    <div className="plr-section" data-testid="guild-history">
      <p className="adm-muted adm-small">{G.history.intro}</p>
      {!canAudit ? <p className="adm-muted adm-small">{G.history.ownOnly}</p> : null}
      <Table
        label="History"
        columns={columns}
        rows={rows ?? []}
        rowKey={(r) => r.id}
        loading={rows === null && error === null}
        error={error}
        emptyTitle={G.history.empty}
      />
      <Button
        variant="ghost"
        onClick={() => {
          navigate({
            category: 'audit',
            tab: 'log',
            params: { groups: 'guild,admin', q: `"guild":${String(id)}` },
          });
        }}
      >
        {G.history.openAudit}
      </Button>
    </div>
  );
}

export type { GuildRank };
