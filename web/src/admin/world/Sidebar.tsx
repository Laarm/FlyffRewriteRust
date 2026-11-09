// The right sidebar of the map (Plan/25 section 3.2): tabs Inspector, Layers, Lists and Tools. The Layers tab (overlay toggles, entity kinds, the
// monster rank and state filters, the text filter) and the Lists tab (live lists of what is in view) are built here; the Inspector and the Tools
// tab are filled by the slots of `views/world/slots.tsx` (the inspector task), with a plain default so that the map works without them.
import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { copyText } from '../app/clipboard';
import { Button } from '../components/Button';
import { Checkbox } from '../components/fields';
import { Tabs, type TabItem } from '../components/Tabs';
import { toast } from '../stores/toasts';
import { layersOfWorld, useLayers } from '../views/world/layers';
import type { EntityInfo, MapSlotProps, MapSlots } from './api';
import { sinceLabel } from './heatmap';
import type { Place } from '../components/mapPlace';
import type { MapSession } from './session';
import { M } from './strings';
import { Flag, Kind, rankName } from './types';
import type { OverlayToggles } from './overlays';
import type { KindToggles, MonsterFilter, SidebarTab } from './state';

export interface SidebarProps {
  readonly session: MapSession;
  readonly slots: MapSlots;
}

const TABS: readonly SidebarTab[] = ['inspector', 'layers', 'lists', 'tools'];

export function Sidebar({ session, slots }: SidebarProps) {
  const tab = useStore(session.store, (s) => s.tab);
  const body = useRef<HTMLDivElement>(null);
  // each tab starts at its top
  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
  }, [tab]);
  const items: TabItem[] = TABS.map((t) => ({ id: t, label: M.tabs[t] }));
  const Inspector = slots.inspector ?? DefaultInspector;
  const Tools = slots.tools ?? DefaultTools;
  const api = { api: session };
  return (
    <div className="map-sidebar">
      <Tabs
        tabs={items}
        value={tab}
        idPrefix="map-side"
        label="Map sidebar"
        onChange={(id) => {
          session.setTab(id as SidebarTab);
        }}
      />
      <div
        ref={body}
        className="map-sidebar__body"
        role="tabpanel"
        id="map-side-panel"
        aria-labelledby={`map-side-tab-${tab}`}
      >
        {tab === 'inspector' ? <Inspector {...api} /> : null}
        {tab === 'layers' ? <LayersPanel session={session} /> : null}
        {tab === 'lists' ? <ListsPanel session={session} /> : null}
        {tab === 'tools' ? <Tools {...api} /> : null}
      </div>
    </div>
  );
}

// ---- Layers -------------------------------------------------------------------------------------------------------------------------

const KIND_KEYS: readonly (keyof KindToggles)[] = [
  'players',
  'monsters',
  'npcs',
  'items',
  'pets',
  'ctrls',
];
const OVERLAY_KEYS: readonly (keyof OverlayToggles)[] = [
  'regions',
  'titles',
  'teleports',
  'portals',
  'spawns',
  'continents',
  'grid',
  'trails',
  'heatmap',
  'atlas',
];

export function LayersPanel({ session }: { readonly session: MapSession }) {
  const kinds = useStore(session.store, (s) => s.kinds);
  const toggles = useStore(session.store, (s) => s.toggles);
  const mf = useStore(session.store, (s) => s.monsterFilter);
  const filterText = useStore(session.store, (s) => s.filterText);
  const filterErrors = useStore(session.store, (s) => s.filterErrors);
  const heatSince = useStore(session.store, (s) => s.heatSince);
  const L = M.layersPanel;
  return (
    <div className="map-panel">
      <h3>{L.entities}</h3>
      <div className="map-panel__checks">
        {KIND_KEYS.map((k) => (
          <Checkbox
            key={k}
            label={L.kinds[k]}
            checked={kinds[k]}
            onChange={(e) => {
              session.setKind(k, e.target.checked);
            }}
          />
        ))}
      </div>
      <h3>{L.monsters}</h3>
      <div className="map-panel__checks">
        {(['normal', 'captain', 'boss'] as const).map((k) => (
          <Checkbox
            key={k}
            label={L.ranks[k]}
            checked={mf[k]}
            onChange={(e) => {
              session.setMonsterFilter(k, e.target.checked);
            }}
          />
        ))}
        {(['awake', 'asleep'] as const).map((k) => (
          <Checkbox
            key={k}
            label={L.states[k]}
            checked={mf[k satisfies keyof MonsterFilter]}
            onChange={(e) => {
              session.setMonsterFilter(k, e.target.checked);
            }}
          />
        ))}
      </div>
      <h3>{L.overlays}</h3>
      <div className="map-panel__checks">
        {OVERLAY_KEYS.map((k) => (
          <Checkbox
            key={k}
            label={
              k === 'heatmap' && heatSince !== null
                ? `${L.toggles.heatmap} (${sinceLabel(heatSince)})`
                : L.toggles[k]
            }
            checked={toggles[k]}
            onChange={(e) => {
              session.setToggle(k, e.target.checked);
            }}
          />
        ))}
      </div>
      <h3>{L.filter}</h3>
      <input
        className="adm-input"
        type="text"
        aria-label={L.filter}
        placeholder="monster:aibatt level>20 state:awake"
        value={filterText}
        onChange={(e) => {
          session.setFilterText(e.target.value);
        }}
      />
      {filterErrors.length > 0 ? (
        <ul className="map-panel__errors" role="alert">
          {filterErrors.map((e) => (
            <li key={e}>{e}</li>
          ))}
        </ul>
      ) : (
        <p className="map-panel__hint">{L.filterHelp}</p>
      )}
      {toggles.heatmap ? <p className="map-panel__hint">{L.heatOff}</p> : null}
    </div>
  );
}

// ---- Lists --------------------------------------------------------------------------------------------------------------------------

type SortKey = 'name' | 'level';

function sortList(list: readonly EntityInfo[], key: SortKey, desc: boolean): EntityInfo[] {
  const out = [...list];
  out.sort(
    (a, b) => (key === 'name' ? a.name.localeCompare(b.name) : a.level - b.level) || a.id - b.id,
  );
  return desc ? out.reverse() : out;
}

/** Groups monsters by name with a count (the "monsters by type" list). */
export function groupMonsters(
  list: readonly EntityInfo[],
): { name: string; prop: number; count: number; first: number }[] {
  const by = new Map<number, { name: string; prop: number; count: number; first: number }>();
  for (const e of list) {
    const g = by.get(e.prop);
    if (g) g.count += 1;
    else by.set(e.prop, { name: e.name, prop: e.prop, count: 1, first: e.id });
  }
  return [...by.values()].sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

export function ListsPanel({ session }: { readonly session: MapSession }) {
  const [rows, setRows] = useState<readonly EntityInfo[]>([]);
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: 'name', desc: false });
  const meta = useStore(session.store, (s) => s.meta);
  const selection = useStore(session.store, (s) => s.selection);
  const L = M.lists;

  // the lists follow the view once a second (never per frame)
  useEffect(() => {
    const read = (): void => {
      setRows(session.inView());
    };
    read();
    const t = setInterval(read, 1000);
    return () => {
      clearInterval(t);
    };
  }, [session]);

  const players = useMemo(
    () =>
      sortList(
        rows.filter((r) => r.kind === Kind.PLAYER),
        sort.key,
        sort.desc,
      ),
    [rows, sort],
  );
  const bosses = useMemo(
    () =>
      sortList(
        rows.filter(
          (r) =>
            r.kind === Kind.MONSTER && ((r.sub >= 2 && r.sub <= 4) || (r.flags & Flag.BOSS) !== 0),
        ),
        sort.key,
        sort.desc,
      ),
    [rows, sort],
  );
  const monsters = useMemo(
    () => groupMonsters(rows.filter((r) => r.kind === Kind.MONSTER)),
    [rows],
  );

  const head = (key: SortKey, label: string): React.ReactNode => (
    <button
      type="button"
      className="map-list__sort"
      aria-label={`${L.sort}: ${label}`}
      onClick={() => {
        setSort((s) => ({ key, desc: s.key === key ? !s.desc : false }));
      }}
    >
      {label}
      {sort.key === key ? (sort.desc ? ' ▾' : ' ▴') : ''}
    </button>
  );

  const entityRow = (e: EntityInfo): React.ReactNode => (
    <tr key={e.id} className={selection.includes(e.id) ? 'is-selected' : undefined}>
      <td>
        <button
          type="button"
          className="map-list__link"
          onClick={() => {
            session.select([e.id]);
          }}
        >
          {e.name}
        </button>
      </td>
      <td className="map-list__num">{e.level}</td>
    </tr>
  );

  const place = (p: Place, label: string): React.ReactNode => (
    <li key={label}>
      <button
        type="button"
        className="map-list__link"
        onClick={() => {
          void session.flyTo(p);
        }}
      >
        {label}
      </button>
    </li>
  );

  return (
    <div className="map-panel">
      <h3>{`${L.players} (${String(players.length)})`}</h3>
      {players.length === 0 ? (
        <p className="map-panel__hint">{L.empty}</p>
      ) : (
        <table className="map-list">
          <thead>
            <tr>
              <th>{head('name', L.name)}</th>
              <th className="map-list__num">{head('level', L.level)}</th>
            </tr>
          </thead>
          <tbody>{players.slice(0, 200).map(entityRow)}</tbody>
        </table>
      )}
      <h3>{`${L.bosses} (${String(bosses.length)})`}</h3>
      {bosses.length === 0 ? (
        <p className="map-panel__hint">{L.empty}</p>
      ) : (
        <table className="map-list">
          <tbody>{bosses.slice(0, 100).map(entityRow)}</tbody>
        </table>
      )}
      <h3>{`${L.monsters} (${String(monsters.reduce((n, g) => n + g.count, 0))})`}</h3>
      {monsters.length === 0 ? (
        <p className="map-panel__hint">{L.empty}</p>
      ) : (
        <table className="map-list">
          <tbody>
            {monsters.slice(0, 60).map((g) => (
              <tr key={g.prop}>
                <td>
                  <button
                    type="button"
                    className="map-list__link"
                    title="Select the monsters of this type in view"
                    onClick={() => {
                      session.select(
                        rows
                          .filter((r) => r.kind === Kind.MONSTER && r.prop === g.prop)
                          .map((r) => r.id),
                      );
                    }}
                  >
                    {g.name}
                  </button>
                </td>
                <td className="map-list__num">{g.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {meta && (meta.portals.length > 0 || meta.teleports.length > 0 || meta.revival.length > 0) ? (
        <>
          <h3>{L.places}</h3>
          <ul className="map-list map-list--places">
            {meta.teleports.map((t) =>
              place({ world: meta.id, layer: 0, x: t.pos[0], z: t.pos[2], upp: 2 }, t.name),
            )}
            {meta.revival.map((r) =>
              place(
                { world: meta.id, layer: 0, x: r.pos[0], z: r.pos[2], upp: 2 },
                `${r.title ?? r.key} (revival)`,
              ),
            )}
            {meta.portals.map((p, i) =>
              place(
                {
                  world: meta.id,
                  layer: 0,
                  x: (p.rect[0] + p.rect[2]) / 2,
                  z: (p.rect[1] + p.rect[3]) / 2,
                  upp: 2,
                },
                `${p.title ?? `Portal ${String(i + 1)}`} → ${p.to_name}`,
              ),
            )}
          </ul>
        </>
      ) : null}
    </div>
  );
}

// ---- defaults for the slots ----------------------------------------------------------------------------------------------------------

/** "3 players, 12 monsters": the counts per kind of a multi-selection. */
export function selectionCounts(list: readonly EntityInfo[]): string {
  const by = new Map<number, number>();
  for (const e of list) by.set(e.kind, (by.get(e.kind) ?? 0) + 1);
  const names: Readonly<Record<number, string>> = {
    1: 'players',
    2: 'monsters',
    3: 'NPCs',
    4: 'pets',
    5: 'items',
    6: 'ctrls',
  };
  return [...by.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([k, n]) => `${String(n)} ${names[k] ?? 'objects'}`)
    .join(', ');
}

/** The plain inspector: what is selected, where, and Follow / Copy `/te` (the inspector task replaces it with the cards of Plan/25 section 3.10). */
export function DefaultInspector({ api }: MapSlotProps) {
  const selection = useStore(api.store, (s) => s.selection);
  const follow = useStore(api.store, (s) => s.follow);
  const worldId = useStore(api.store, (s) => s.worldId);
  const counts = useStore(api.store, (s) => s.counts);
  const layers = useLayers((s) => s.layers);
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => {
      tick((n) => n + 1);
    }, 1000);
    return () => {
      clearInterval(t);
    };
  }, []);
  const I = M.inspector;
  if (selection.length === 0) {
    return (
      <div className="map-panel">
        <h3>{I.summary}</h3>
        <dl className="map-facts">
          <dt>{I.nothing}</dt>
          <dd>{I.playersHere(counts.players)}</dd>
          <dd>{I.instancesLive(layersOfWorld(layers, worldId).length)}</dd>
        </dl>
      </div>
    );
  }
  const first = api.entity(selection[0] ?? 0);
  const many =
    selection.length > 1 ? selection.map((id) => api.entity(id)).filter((e) => e !== null) : [];
  return (
    <div className="map-panel">
      <h3>{I.selected(selection.length)}</h3>
      {many.length > 0 ? (
        <>
          <p className="map-panel__hint" data-testid="selection-counts">
            {selectionCounts(many)}
          </p>
          <table className="map-list" data-testid="selection-list">
            <tbody>
              {many.slice(0, 100).map((e) => (
                <tr key={e.id}>
                  <td>
                    <button
                      type="button"
                      className="map-list__link"
                      onClick={() => {
                        api.select([e.id]);
                      }}
                    >
                      {e.name}
                    </button>
                  </td>
                  <td className="map-list__num">{e.level}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      ) : null}
      {first ? (
        <dl className="map-facts" data-testid="inspector-card">
          <dt>{I.kind[first.kind] ?? 'Object'}</dt>
          <dd className="map-facts__name">{first.name}</dd>
          {first.kind === Kind.PLAYER || first.kind === Kind.MONSTER ? (
            <>
              <dt>{I.level}</dt>
              <dd>{first.level}</dd>
            </>
          ) : null}
          {first.kind === Kind.MONSTER ? (
            <>
              <dt>Rank</dt>
              <dd>{rankName(first.sub)}</dd>
            </>
          ) : null}
          {first.kind === Kind.PLAYER || first.kind === Kind.MONSTER ? (
            <>
              <dt>{I.hp}</dt>
              <dd>{`${String(first.hpPct)} %`}</dd>
            </>
          ) : null}
          <dt>{I.position}</dt>
          <dd>{`${first.x.toFixed(1)}, ${first.z.toFixed(1)}`}</dd>
        </dl>
      ) : (
        <p className="map-panel__hint">{I.notInView}</p>
      )}
      <div className="map-panel__actions">
        {first ? (
          <>
            <Button
              size="sm"
              variant={follow?.objId === first.id ? 'primary' : 'secondary'}
              onClick={() => {
                api.follow(follow?.objId === first.id ? null : first.id);
              }}
            >
              {follow?.objId === first.id ? I.unfollow : I.followAction}
            </Button>
            <Button
              size="sm"
              onClick={() => {
                const text = M.copyTe(worldId, first.x, first.z);
                if (copyText(text)) toast.success(M.copied(text));
                else toast.error(new Error('Copying is not available here.'), 'Copy failed');
              }}
            >
              {I.copyTe}
            </Button>
          </>
        ) : null}
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            api.clearSelection();
          }}
        >
          {I.clear}
        </Button>
      </div>
    </div>
  );
}

/** The plain tools tab: the ruler and the box selection (the action tools are the inspector task's). */
export function DefaultTools({ api }: MapSlotProps) {
  const tool = useStore(api.store, (s) => s.tool);
  const ruler = useStore(api.store, (s) => s.ruler);
  const T = M.tools;
  const set = (t: 'pan' | 'select' | 'ruler'): void => {
    api.setTool(t);
  };
  return (
    <div className="map-panel">
      <h3>{T.heading}</h3>
      <div className="map-panel__actions">
        <Button
          size="sm"
          variant={tool === 'pan' ? 'primary' : 'secondary'}
          onClick={() => {
            set('pan');
          }}
        >
          {T.pan}
        </Button>
        <Button
          size="sm"
          variant={tool === 'select' ? 'primary' : 'secondary'}
          onClick={() => {
            set('select');
          }}
        >
          {T.boxSelect}
        </Button>
        <Button
          size="sm"
          variant={tool === 'ruler' ? 'primary' : 'secondary'}
          onClick={() => {
            set('ruler');
          }}
        >
          {T.ruler}
        </Button>
      </div>
      {tool === 'ruler' ? <p className="map-panel__hint">{ruler?.b ? '' : T.rulerHint}</p> : null}
      <p className="map-panel__hint">{T.soon}</p>
    </div>
  );
}
