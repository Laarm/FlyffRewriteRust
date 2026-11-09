// World & Content -> Spawns (Plan/25 section 7.2): the respawn lines of a world (what spawns where, the target, how many are alive and awake, the
// time to the next refill) with "Respawn now" for a line, and the monster census (alive per rank, asleep or awake, the 20 most numerous types).
// The counters are those of the main layer. Read from `GET /admin/api/spawns` and `GET /admin/api/census`.
import { useEffect, useMemo, useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Button,
  Checkbox,
  ErrorState,
  Field,
  Select,
  Skeleton,
  Table,
  TextInput,
  type Column,
} from '../../components';
import { loadWorlds } from '../../components/mapMeta';
import type { MonsterCensusDto, SpawnRowDto, SpawnTableDto } from '../../gen/dto';
import { navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import type { WorldsDoc } from '../../world/metaTypes';
import { useSnapshot } from '../dashboard/useSnapshot';
import { monsterCensus, respawnNow, spawnTable } from './api';
import { T } from './strings';

const S = T.spawns;
const C = T.census;

/** The main world (Madrigal). */
export const DEFAULT_WORLD = 1;

export function rankName(rank: number): string {
  return C.ranks[rank] ?? `Rank ${String(rank)}`;
}

/** `full`, `due now`, `paused ...`, `12 min`: when a line refills next. */
export function nextRespawn(row: SpawnRowDto, respawnOn: boolean): string {
  if (!respawnOn) return '-';
  if (row.alive >= row.target) return S.full;
  if (!row.in_window) return S.paused;
  if (row.countdown_s <= 0) return S.due;
  return S.countdown(row.countdown_s);
}

/** The point to centre the map on: the middle of the line's rectangle. */
export function centreOf(rect: readonly [number, number, number, number]): {
  x: number;
  z: number;
} {
  return { x: Math.round((rect[0] + rect[2]) / 2), z: Math.round((rect[1] + rect[3]) / 2) };
}

/** Which lines the filters let through. */
export function visibleLines(
  rows: readonly SpawnRowDto[],
  filter: { readonly text: string; readonly onlyMissing: boolean; readonly onlyMonsters: boolean },
): readonly SpawnRowDto[] {
  const needle = filter.text.trim().toLowerCase();
  return rows.filter(
    (r) =>
      (!filter.onlyMonsters || r.kind === 'monster') &&
      (!filter.onlyMissing || r.alive < r.target) &&
      (needle === '' || r.name.toLowerCase().includes(needle) || String(r.prop) === needle),
  );
}

function Bar({ value, max }: { readonly value: number; readonly max: number }) {
  const pct = max <= 0 ? 0 : Math.min(100, Math.round((value / max) * 100));
  return (
    <span className="content-bar" role="presentation">
      <span style={{ width: `${String(pct)}%` }} />
    </span>
  );
}

function SpawnLines({ world, table }: { readonly world: number; readonly table: SpawnTableDto }) {
  const canEdit = useCan('act.world');
  const [text, setText] = useState('');
  const [onlyMissing, setOnlyMissing] = useState(false);
  const [onlyMonsters, setOnlyMonsters] = useState(true);
  const rows = visibleLines(table.lines, { text, onlyMissing, onlyMonsters });

  const respawn = (row: SpawnRowDto): void => {
    const name = row.name === '' ? `#${String(row.region)}` : row.name;
    void runConfirmed({
      title: S.respawnTitle(`${name} (line ${String(row.region)})`),
      message: S.respawnBody,
      confirmLabel: S.respawnNow,
      reason: 'optional',
      run: (given) => respawnNow(row.region, world, table.layer, given.reason),
      success: (r) => S.respawned(name, r.affected ?? 0),
    });
  };

  const columns: Column<SpawnRowDto>[] = [
    {
      id: 'line',
      header: S.line,
      cell: (r) => r.region,
      text: (r) => r.region,
      sortable: true,
      sortValue: (r) => r.region,
      align: 'right',
      width: '60px',
    },
    {
      id: 'what',
      header: S.what,
      cell: (r) => (
        <span className="content-wrap content-what">
          <strong>{r.name === '' ? `#${String(r.prop)}` : r.name}</strong>
          {r.kind === 'monster' && <span className="adm-muted"> Lv {r.level}</span>}{' '}
          {r.kind !== 'monster' && <Badge>{r.kind}</Badge>}
          {r.rank > 0 && <Badge tone="warn">{rankName(r.rank)}</Badge>}
          {r.fixed && <Badge>{S.fixed}</Badge>}
          {r.flying && <Badge>{S.flying}</Badge>}
        </span>
      ),
      text: (r) => r.name,
      sortable: true,
      sortValue: (r) => r.name,
    },
    {
      id: 'area',
      header: S.area,
      cell: (r) => {
        const c = centreOf(r.rect);
        return (
          <span className="content-area">
            <code className="adm-muted">
              {String(r.rect[0])},{String(r.rect[1])} - {String(r.rect[2])},{String(r.rect[3])}
            </code>
            <Button
              size="sm"
              variant="ghost"
              aria-label={`${S.openMap}: ${r.name} line ${String(r.region)}`}
              onClick={() => {
                navigate({
                  category: 'world',
                  tab: 'map',
                  params: {
                    w: String(world),
                    l: String(table.layer),
                    x: String(c.x),
                    z: String(c.z),
                  },
                });
              }}
            >
              {S.mapShort}
            </Button>
          </span>
        );
      },
      text: (r) => r.rect.join(','),
      width: '230px',
    },
    {
      id: 'target',
      header: S.target,
      cell: (r) => r.target,
      text: (r) => r.target,
      align: 'right',
      sortable: true,
      sortValue: (r) => r.target,
      width: '70px',
    },
    {
      id: 'alive',
      header: S.alive,
      cell: (r) => (
        <span className="content-alive">
          <span className={r.alive < r.target ? 'content-low' : ''}>{r.alive}</span>
          <Bar value={r.alive} max={r.target} />
        </span>
      ),
      text: (r) => r.alive,
      sortable: true,
      sortValue: (r) => r.alive,
      align: 'right',
      width: '100px',
    },
    {
      id: 'awake',
      header: S.awake,
      cell: (r) => r.awake,
      text: (r) => r.awake,
      sortable: true,
      sortValue: (r) => r.awake,
      align: 'right',
      width: '70px',
    },
    {
      id: 'aggressive',
      header: S.aggressive,
      cell: (r) =>
        r.aggressive ? (
          <span title={`${String(r.aggressive_alive)} alive`}>{r.aggressive_alive}</span>
        ) : (
          <span className="adm-muted">-</span>
        ),
      text: (r) => r.aggressive_alive,
      align: 'right',
      width: '85px',
    },
    {
      id: 'next',
      header: S.next,
      cell: (r) => nextRespawn(r, table.respawn_on),
      text: (r) => nextRespawn(r, table.respawn_on),
      width: '135px',
    },
  ];
  if (canEdit) {
    columns.push({
      id: 'respawn',
      header: S.actions,
      cell: (r) => (
        <Button
          size="sm"
          aria-label={`${S.respawnNow}: ${r.name} line ${String(r.region)}`}
          disabled={!table.respawn_on || r.kind === 'item'}
          onClick={() => {
            respawn(r);
          }}
        >
          {S.respawnNow}
        </Button>
      ),
      width: '115px',
    });
  }

  return (
    <section className="content-section" aria-label="Respawn lines">
      <div className="content-toolbar">
        <Field label={S.find}>
          <TextInput
            value={text}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
        </Field>
        <Checkbox
          label={S.onlyMissing}
          checked={onlyMissing}
          onChange={(e) => {
            setOnlyMissing(e.target.checked);
          }}
        />
        <Checkbox
          label={S.onlyMonsters}
          checked={onlyMonsters}
          onChange={(e) => {
            setOnlyMonsters(e.target.checked);
          }}
        />
        <span className="adm-muted">
          {S.lines(rows.length)} - {S.gameTime(table.game_day, table.game_hour)} {S.layerNote}
        </span>
      </div>
      {!table.respawn_on && (
        <div className="adm-field__error" role="alert">
          {S.off}
        </div>
      )}
      <Table
        label="Respawn lines"
        columns={columns}
        rows={rows}
        rowKey={(r) => String(r.region)}
        emptyTitle={S.empty}
        emptyHint={S.emptyHint}
        exportName={`spawns-world-${String(world)}`}
      />
    </section>
  );
}

function Census({ census }: { readonly census: MonsterCensusDto }) {
  const cards: readonly { readonly label: string; readonly value: number }[] = [
    { label: C.alive, value: census.alive },
    { label: C.awake, value: census.awake },
    { label: C.asleep, value: census.asleep },
    { label: C.players, value: census.players },
    { label: C.npcs, value: census.npcs },
    { label: C.layers, value: census.layers },
  ];
  const rankColumns: Column<MonsterCensusDto['by_rank'][number]>[] = [
    { id: 'rank', header: C.rank, cell: (r) => rankName(r.rank), text: (r) => rankName(r.rank) },
    { id: 'alive', header: C.alive, cell: (r) => r.alive, text: (r) => r.alive, align: 'right' },
    { id: 'awake', header: C.awake, cell: (r) => r.awake, text: (r) => r.awake, align: 'right' },
    {
      id: 'asleep',
      header: C.asleep,
      cell: (r) => r.asleep,
      text: (r) => r.asleep,
      align: 'right',
    },
    {
      id: 'bar',
      header: '',
      cell: (r) => <Bar value={r.alive} max={census.alive} />,
      width: '140px',
    },
  ];
  const topColumns: Column<MonsterCensusDto['top'][number]>[] = [
    {
      id: 'type',
      header: C.type,
      cell: (t) => (
        <span>
          <strong>{t.name === '' ? `#${String(t.prop)}` : t.name}</strong>{' '}
          {t.rank > 0 && <Badge tone="warn">{rankName(t.rank)}</Badge>}
        </span>
      ),
      text: (t) => t.name,
    },
    {
      id: 'level',
      header: C.level,
      cell: (t) => t.level,
      text: (t) => t.level,
      align: 'right',
      width: '70px',
    },
    {
      id: 'alive',
      header: C.alive,
      cell: (t) => t.alive,
      text: (t) => t.alive,
      align: 'right',
      width: '80px',
    },
    {
      id: 'awake',
      header: C.awake,
      cell: (t) => t.awake,
      text: (t) => t.awake,
      align: 'right',
      width: '80px',
    },
  ];
  return (
    <section className="content-section" aria-label={C.title}>
      <h3 className="content-h">{C.title}</h3>
      <p className="adm-muted">{C.hint}</p>
      <div className="content-cards">
        {cards.map((c) => (
          <div key={c.label} className="content-card">
            <h4>{c.label}</h4>
            <div className="content-card__value">{c.value.toLocaleString('en-US')}</div>
          </div>
        ))}
      </div>
      <div className="content-two">
        <Table
          label={C.rank}
          columns={rankColumns}
          rows={census.by_rank}
          rowKey={(r) => String(r.rank)}
        />
        <div>
          <h4 className="content-h4">{C.top}</h4>
          <Table
            label={C.top}
            columns={topColumns}
            rows={census.top}
            rowKey={(t) => String(t.prop)}
          />
        </div>
      </div>
    </section>
  );
}

export function SpawnsTab({ route }: TabProps) {
  const [worlds, setWorlds] = useState<WorldsDoc | null>(null);
  const wanted = Number(route.params['w']);
  const [world, setWorld] = useState<number>(
    Number.isInteger(wanted) && wanted > 0 ? wanted : DEFAULT_WORLD,
  );
  useEffect(() => {
    let live = true;
    loadWorlds({ fresh: true }).then(
      (doc) => {
        if (live) setWorlds(doc);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  const table = useSnapshot(
    (signal) => spawnTable(world, signal),
    `spawns|${String(world)}`,
    5_000,
  );
  const census = useSnapshot(
    (signal) => monsterCensus(world, signal),
    `census|${String(world)}`,
    10_000,
  );
  const groups = useMemo(
    () =>
      worlds === null
        ? []
        : worlds.groups.map((g) => ({
            id: g.id,
            label: g.label,
            worlds: worlds.worlds.filter((w) => w.group === g.id),
          })),
    [worlds],
  );

  return (
    <div className="content-tab">
      <div className="content-toolbar">
        <Field label={S.world}>
          <Select
            value={String(world)}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isInteger(n) && n > 0) setWorld(n);
            }}
          >
            {worlds === null && <option value={String(world)}>{`World ${String(world)}`}</option>}
            {groups.map(
              (g) =>
                g.worlds.length > 0 && (
                  <optgroup key={g.id} label={g.label}>
                    {g.worlds.map((w) => (
                      <option key={w.id} value={String(w.id)}>
                        {w.name} ({String(w.online)} online)
                      </option>
                    ))}
                  </optgroup>
                ),
            )}
          </Select>
        </Field>
      </div>
      {table.error !== null && table.data === null ? (
        <ErrorState error={table.error} onRetry={table.reload} />
      ) : table.data === null ? (
        <Skeleton lines={8} />
      ) : (
        <SpawnLines world={world} table={table.data} />
      )}
      {census.error !== null && census.data === null ? (
        <ErrorState
          error={census.error}
          onRetry={census.reload}
          title="Could not load the census"
        />
      ) : census.data === null ? (
        <Skeleton lines={4} />
      ) : (
        <Census census={census.data} />
      )}
    </div>
  );
}
