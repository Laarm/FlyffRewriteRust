// Guilds -> Guilds (Plan/25 section 6.1): the guild list from the game thread (`GET /admin/api/guilds`, server cursor paging and sort, CSV
// export): logo, name, level, master, members online/total (max), penya, contribution, war, house, created; filters name, level range and
// "at war". A click opens the drawer (`?g=<id>`, a deep link).
import { useState } from 'react';
import { withRecord } from '../../app/route';
import type { TabProps } from '../../app/views';
import { GuildLogo } from '../../components/GuildLogo';
import {
  Field,
  NumberInput,
  Select,
  Table,
  TextInput,
  usePagedList,
  type Column,
  type SortState,
} from '../../components';
import type { GuildRow } from '../../gen/dto';
import { getRoute, navigate } from '../../stores/route';
import { formatDateTime, formatNumber } from '../players/format';
import { useDebounced } from '../players/hooks';
import { openCharacter } from '../players/rowMenu';
import { useGuildsChanged } from './changes';
import { G } from './strings';

type WarFilter = 'any' | 'yes' | 'no';

const SORT_KEYS: Readonly<Record<string, string>> = {
  name: 'name',
  level: 'level',
  members: 'members',
  gold: 'gold',
  pxp: 'pxp',
  created: 'created',
};

/** Opens the drawer of guild `id` (the deep link `#/guilds?g=<id>`). */
export function openGuild(id: number): void {
  const route = getRoute() ?? { category: 'guilds', tab: 'guilds', params: {} };
  navigate(withRecord(route, { kind: 'guild', id: String(id) }));
}

export function guildName(g: Pick<GuildRow, 'id' | 'name' | 'named'>): string {
  return g.named ? g.name : G.list.unnamed(g.id);
}

export function GuildsTab({ route }: TabProps) {
  const [q, setQ] = useState('');
  const [levelMin, setLevelMin] = useState('');
  const [levelMax, setLevelMax] = useState('');
  const [war, setWar] = useState<WarFilter>('any');
  const [sort, setSort] = useState<SortState | null>(null);
  const text = useDebounced(q.trim());
  const sortKey = sort ? SORT_KEYS[sort.column] : undefined;
  const list = usePagedList<GuildRow>({
    path: '/admin/api/guilds',
    query: {
      ...(text === '' ? {} : { q: text }),
      ...(levelMin === '' ? {} : { level_min: Number(levelMin) }),
      ...(levelMax === '' ? {} : { level_max: Number(levelMax) }),
      ...(war === 'any' ? {} : { at_war: war === 'yes' }),
      ...(sortKey ? { sort: sortKey, dir: sort?.dir ?? 'asc' } : {}),
    },
    limit: 100,
  });
  useGuildsChanged(list.reload);
  const columns: Column<GuildRow>[] = [
    {
      id: 'logo',
      header: G.list.columns.logo,
      width: '64px',
      cell: (g) => <GuildLogo logo={g.logo} scale={2} />,
      text: (g) => g.logo,
    },
    {
      id: 'name',
      header: G.list.columns.name,
      sortable: true,
      cell: (g) => (g.named ? <strong>{g.name}</strong> : <em>{guildName(g)}</em>),
      text: (g) => guildName(g),
    },
    {
      id: 'level',
      header: G.list.columns.level,
      sortable: true,
      align: 'right',
      cell: (g) => g.level,
      text: (g) => g.level,
    },
    {
      id: 'master',
      header: G.list.columns.master,
      cell: (g) =>
        g.master ? (
          <button
            type="button"
            className="adm-linkbtn"
            onClick={(e) => {
              e.stopPropagation();
              if (g.master) openCharacter(g.master.id);
            }}
          >
            {g.master.name}
          </button>
        ) : (
          '-'
        ),
      text: (g) => g.master?.name ?? '',
    },
    {
      id: 'members',
      header: G.list.columns.members,
      sortable: true,
      align: 'right',
      cell: (g) => `${String(g.online)}/${String(g.members)} (${String(g.max_members)})`,
      text: (g) => `${String(g.online)}/${String(g.members)} (${String(g.max_members)})`,
      sortValue: (g) => g.members,
    },
    {
      id: 'gold',
      header: G.list.columns.gold,
      sortable: true,
      align: 'right',
      cell: (g) => formatNumber(g.gold),
      text: (g) => g.gold,
    },
    {
      id: 'pxp',
      header: G.list.columns.pxp,
      sortable: true,
      align: 'right',
      cell: (g) => formatNumber(g.pxp),
      text: (g) => g.pxp,
    },
    {
      id: 'war',
      header: G.list.columns.war,
      cell: (g) =>
        g.war ? (
          <button
            type="button"
            className="adm-linkbtn"
            onClick={(e) => {
              e.stopPropagation();
              if (g.war) openGuild(g.war.enemy_id);
            }}
          >
            {g.war.enemy_name === '' ? `#${String(g.war.enemy_id)}` : g.war.enemy_name}
          </button>
        ) : (
          G.list.noWar
        ),
      text: (g) => g.war?.enemy_name ?? '',
    },
    { id: 'house', header: G.list.columns.house, cell: () => G.list.house, text: () => '' },
    {
      id: 'created',
      header: G.list.columns.created,
      sortable: true,
      cell: (g) => formatDateTime(g.created_at),
      text: (g) => g.created_at,
    },
  ];
  return (
    <div className="plr-tab" data-testid="guilds-tab">
      <div className="plr-bar-top">
        <Field label={G.list.search}>
          <TextInput
            type="search"
            value={q}
            autoFocus
            onChange={(e) => {
              setQ(e.target.value);
            }}
          />
        </Field>
        <Field label={G.list.levelMin}>
          <NumberInput
            min={1}
            max={50}
            value={levelMin}
            onChange={(e) => {
              setLevelMin(e.target.value);
            }}
          />
        </Field>
        <Field label={G.list.levelMax}>
          <NumberInput
            min={1}
            max={50}
            value={levelMax}
            onChange={(e) => {
              setLevelMax(e.target.value);
            }}
          />
        </Field>
        <Field label={G.list.atWar}>
          <Select
            value={war}
            onChange={(e) => {
              setWar(e.target.value as WarFilter);
            }}
          >
            <option value="any">{G.list.atWarAny}</option>
            <option value="yes">{G.list.atWarYes}</option>
            <option value="no">{G.list.atWarNo}</option>
          </Select>
        </Field>
      </div>
      <Table
        label="Guilds"
        columns={columns}
        rows={list.rows}
        rowKey={(g) => String(g.id)}
        loading={list.loading}
        error={list.error}
        onRetry={list.reload}
        hasMore={list.hasMore}
        loadingMore={list.loadingMore}
        onLoadMore={list.loadMore}
        emptyTitle={G.list.empty}
        emptyHint={G.list.emptyHint}
        sort={sort}
        onSortChange={setSort}
        onRowActivate={(g) => {
          openGuild(g.id);
        }}
        selectedKey={route.params['g'] ?? null}
        exportName="guilds"
      />
    </div>
  );
}
