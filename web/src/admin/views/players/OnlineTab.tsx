// Players -> Online (Plan/25 section 4.1): the live table of the `RosterSubscribe` stream (deltas every 2 s) with filters, a row menu
// and bulk actions. Session age, the lingering state and the address (with `view.pii`) come from `GET /admin/api/sessions`, read every
// 10 s and merged by character id.
import { useEffect, useMemo, useState } from 'react';
import { rest } from '../../api/rest';
import { useNow } from '../../app/polling';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Button,
  Checkbox,
  ErrorState,
  Field,
  NumberInput,
  Select,
  Table,
  TextInput,
  type Column,
} from '../../components';
import { useCan } from '../../stores/session';
import { kickMany, message, showOnMap, type Target } from './characterActions';
import { authorityName, formatPos, formatSpan, isStaffLetter } from './format';
import { JobIcon } from './JobIcon';
import { jobName } from './jobs';
import { useRoster, useRosterFeed, type RosterRow } from './roster';
import { openCharacter, RowMenu } from './rowMenu';
import { T } from './strings';
import { GuildChip } from '../guilds/GuildChip';

interface SessionRow {
  readonly character_id: number;
  readonly in_world_since: number;
  readonly lingering: boolean;
  readonly ip?: string;
}

interface SessionsAnswer {
  readonly in_world: readonly SessionRow[];
}

export interface OnlineFilters {
  readonly text: string;
  readonly world: string;
  readonly minLevel: number;
  readonly maxLevel: number;
  readonly job: string;
  readonly staffOnly: boolean;
}

export const NO_ONLINE_FILTERS: OnlineFilters = {
  text: '',
  world: '',
  minLevel: 0,
  maxLevel: 0,
  job: '',
  staffOnly: false,
};

/** The roster rows that pass the filters (the text matches the name or the character id). */
export function filterRoster(rows: readonly RosterRow[], f: OnlineFilters): RosterRow[] {
  const text = f.text.trim().toLowerCase();
  return rows.filter((r) => {
    if (text !== '' && !r.name.toLowerCase().includes(text) && String(r.characterId) !== text)
      return false;
    if (f.world !== '' && String(r.world) !== f.world) return false;
    if (f.minLevel > 0 && r.level < f.minLevel) return false;
    if (f.maxLevel > 0 && r.level > f.maxLevel) return false;
    if (f.job !== '' && String(r.job) !== f.job) return false;
    if (f.staffOnly && !isStaffLetter(r.authority)) return false;
    return true;
  });
}

export function targetOf(r: RosterRow): Target {
  return {
    id: r.characterId,
    name: r.name,
    online: true,
    objId: r.objId,
    world: r.world,
    layer: r.layer,
    x: r.x,
    z: r.z,
  };
}

function useSessions(enabled: boolean): ReadonlyMap<number, SessionRow> {
  const [map, setMap] = useState<ReadonlyMap<number, SessionRow>>(new Map());
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    const load = (): void => {
      rest.get<SessionsAnswer>('/admin/api/sessions').then(
        (a) => {
          if (live) setMap(new Map(a.in_world.map((s) => [s.character_id, s])));
        },
        () => {
          // the columns that need it stay empty; the roster itself is live
        },
      );
    };
    load();
    const id = setInterval(load, 10_000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [enabled]);
  return map;
}

export function OnlineTab({ route }: TabProps) {
  useRosterFeed(true);
  const roster = useRoster();
  const sessions = useSessions(true);
  const now = Math.floor(useNow(1000) / 1000);
  const canMessage = useCan('act.message.player');
  const canKick = useCan('act.kick');
  const [filters, setFilters] = useState<OnlineFilters>(NO_ONLINE_FILTERS);
  const [picked, setPicked] = useState<ReadonlySet<number>>(new Set());
  const all = useMemo(
    () => [...roster.rows.values()].sort((a, b) => a.name.localeCompare(b.name)),
    [roster.rows],
  );
  const rows = useMemo(() => filterRoster(all, filters), [all, filters]);
  const worlds = useMemo(() => [...new Set(all.map((r) => r.world))].sort((a, b) => a - b), [all]);
  const jobs = useMemo(() => [...new Set(all.map((r) => r.job))].sort((a, b) => a - b), [all]);
  const set = (patch: Partial<OnlineFilters>): void => {
    setFilters((f) => ({ ...f, ...patch }));
  };
  const chosen = rows.filter((r) => picked.has(r.characterId));
  const toggle = (id: number): void => {
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  };

  const columns: Column<RosterRow>[] = [
    {
      id: 'pick',
      header: '',
      width: '36px',
      cell: (r) => (
        <span
          onClick={(e) => {
            e.stopPropagation();
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
          }}
        >
          <Checkbox
            label=""
            aria-label={`Select ${r.name}`}
            checked={picked.has(r.characterId)}
            onChange={() => {
              toggle(r.characterId);
            }}
          />
        </span>
      ),
    },
    {
      id: 'name',
      header: T.online.columns.name,
      sortable: true,
      cell: (r) => (
        <span className="adm-row">
          <JobIcon job={r.job} size={20} />
          <strong>{r.name}</strong>
        </span>
      ),
      text: (r) => r.name,
    },
    {
      id: 'level',
      header: T.online.columns.level,
      sortable: true,
      align: 'right',
      cell: (r) => r.level,
      text: (r) => r.level,
    },
    {
      id: 'job',
      header: T.online.columns.job,
      sortable: true,
      cell: (r) => jobName(r.job),
      text: (r) => jobName(r.job),
    },
    {
      id: 'guild',
      header: T.online.columns.guild,
      sortable: true,
      cell: (r) => (r.guildId === 0 ? '-' : <GuildChip id={r.guildId} />),
      text: (r) => r.guildId,
    },
    {
      id: 'world',
      header: T.online.columns.world,
      sortable: true,
      cell: (r) => (r.layer === 0 ? String(r.world) : `${String(r.world)} / ${String(r.layer)}`),
      text: (r) => r.world * 1000 + r.layer,
    },
    {
      id: 'position',
      header: T.online.columns.position,
      cell: (r) => (
        <Button
          size="sm"
          variant="ghost"
          onClick={(e) => {
            e.stopPropagation();
            showOnMap(targetOf(r));
          }}
          title={T.actions.showOnMap}
        >
          {formatPos(r.x, r.z)}
        </Button>
      ),
      text: (r) => formatPos(r.x, r.z),
    },
    {
      id: 'age',
      header: T.online.columns.age,
      sortable: true,
      cell: (r) => {
        const s = sessions.get(r.characterId);
        return s ? formatSpan(now - s.in_world_since) : '-';
      },
      text: (r) => sessions.get(r.characterId)?.in_world_since ?? 0,
    },
    {
      id: 'authority',
      header: T.online.columns.authority,
      sortable: true,
      cell: (r) =>
        r.authority === 'F' ? '-' : <Badge tone="accent">{authorityName(r.authority)}</Badge>,
      text: (r) => r.authority,
    },
    {
      id: 'state',
      header: T.online.columns.state,
      cell: (r) =>
        sessions.get(r.characterId)?.lingering ? (
          <Badge tone="warn">{T.common.lingering}</Badge>
        ) : (
          <Badge tone="ok">{T.common.online}</Badge>
        ),
      text: (r) => (sessions.get(r.characterId)?.lingering ? T.common.lingering : T.common.online),
    },
    {
      id: 'ip',
      header: T.online.columns.ip,
      pii: true,
      cell: (r) => sessions.get(r.characterId)?.ip ?? '-',
      text: (r) => sessions.get(r.characterId)?.ip ?? '',
    },
    { id: 'actions', header: '', width: '48px', cell: (r) => <RowMenu target={targetOf(r)} /> },
  ];

  return (
    <div className="plr-tab" data-testid="online-tab">
      <div className="plr-bar-top">
        <Field label={T.online.filterText}>
          <TextInput
            type="search"
            value={filters.text}
            onChange={(e) => {
              set({ text: e.target.value });
            }}
          />
        </Field>
        <Field label={T.online.filterWorld}>
          <Select
            value={filters.world}
            onChange={(e) => {
              set({ world: e.target.value });
            }}
          >
            <option value="">{T.online.allWorlds}</option>
            {worlds.map((w) => (
              <option key={w} value={w}>
                {w}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={T.online.minLevel}>
          <NumberInput
            min={0}
            value={filters.minLevel === 0 ? '' : filters.minLevel}
            onChange={(e) => {
              set({ minLevel: Math.max(0, Math.trunc(Number(e.target.value)) || 0) });
            }}
          />
        </Field>
        <Field label={T.online.maxLevel}>
          <NumberInput
            min={0}
            value={filters.maxLevel === 0 ? '' : filters.maxLevel}
            onChange={(e) => {
              set({ maxLevel: Math.max(0, Math.trunc(Number(e.target.value)) || 0) });
            }}
          />
        </Field>
        <Field label={T.online.job}>
          <Select
            value={filters.job}
            onChange={(e) => {
              set({ job: e.target.value });
            }}
          >
            <option value="">{T.online.anyJob}</option>
            {jobs.map((j) => (
              <option key={j} value={j}>
                {jobName(j)}
              </option>
            ))}
          </Select>
        </Field>
        <Checkbox
          label={T.online.staffOnly}
          checked={filters.staffOnly}
          onChange={(e) => {
            set({ staffOnly: e.target.checked });
          }}
        />
      </div>
      <p className="adm-muted adm-small" data-testid="online-counts">
        {T.online.counts(roster.inWorld, roster.lobby)}
      </p>
      {roster.phase === 'error' ? (
        <ErrorState error={new Error(roster.error ?? '')} title={T.online.streamFailed} />
      ) : null}
      <Table
        label={T.online.title}
        columns={columns}
        rows={rows}
        rowKey={(r) => String(r.characterId)}
        loading={roster.phase === 'loading'}
        emptyTitle={all.length === 0 ? T.online.empty : T.online.emptyFiltered}
        onRowActivate={(r) => {
          openCharacter(r.characterId);
        }}
        selectedKey={route.params['c'] ?? null}
        exportName="online-players"
        toolbar={
          chosen.length > 0 ? (
            <span className="adm-row">
              <strong>{T.online.selected(chosen.length)}</strong>
              {canMessage ? (
                <Button size="sm" onClick={() => void message(chosen.map(targetOf))}>
                  {T.online.bulkMessage}
                </Button>
              ) : null}
              {canKick ? (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => void kickMany(chosen.map(targetOf))}
                >
                  {T.online.bulkKick}
                </Button>
              ) : null}
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setPicked(new Set());
                }}
              >
                {T.online.clearSelection}
              </Button>
            </span>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setPicked(new Set(rows.map((r) => r.characterId)));
              }}
              disabled={rows.length === 0}
            >
              {T.online.selectAll}
            </Button>
          )
        }
      />
    </div>
  );
}
