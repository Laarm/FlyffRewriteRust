// The toolbar of the map (Plan/25 section 3.2): grouped world selector with the players per world, layer selector with the live layers and their
// owners, find on map, Layers, Follow, box Select, refresh and fullscreen. It reads the store of the map and calls the controller.
import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import { Select } from '../components/fields';
import { layersOfWorld, useLayers } from '../views/world/layers';
import type { FindHit, WorldRow } from './metaTypes';
import type { MapSession } from './session';
import { M } from './strings';

export interface ToolbarProps {
  readonly session: MapSession;
  /** The element that goes fullscreen. */
  readonly fullscreenRef: React.RefObject<HTMLElement | null>;
  readonly findRef: React.RefObject<HTMLInputElement | null>;
}

/** The `<option>`s of the world selector: grouped, worlds without tiles and players behind "Show all". */
export function worldOptions(
  worlds: readonly WorldRow[],
  groups: readonly { id: string; label: string }[],
  current: number,
  showAll: boolean,
): { group: string; label: string; rows: WorldRow[] }[] {
  const out: { group: string; label: string; rows: WorldRow[] }[] = [];
  for (const g of groups) {
    const rows = worlds.filter(
      (w) => w.group === g.id && (showAll || w.tiles !== null || w.online > 0 || w.id === current),
    );
    if (rows.length > 0) out.push({ group: g.id, label: g.label, rows });
  }
  return out;
}

export function Toolbar({ session, fullscreenRef, findRef }: ToolbarProps) {
  const store = session.store;
  const worlds = useStore(store, (s) => s.worlds);
  const worldId = useStore(store, (s) => s.worldId);
  const layer = useStore(store, (s) => s.layer);
  const follow = useStore(store, (s) => s.follow);
  const tool = useStore(store, (s) => s.tool);
  const selection = useStore(store, (s) => s.selection);
  const layers = useLayers((s) => s.layers);
  const [showAll, setShowAll] = useState(false);
  const [full, setFull] = useState(false);

  const options = useMemo(
    () => (worlds ? worldOptions(worlds.worlds, worlds.groups, worldId, showAll) : []),
    [worlds, worldId, showAll],
  );
  const live = layersOfWorld(layers, worldId);

  useEffect(() => {
    const onChange = (): void => {
      setFull(document.fullscreenElement !== null);
    };
    document.addEventListener('fullscreenchange', onChange);
    return () => {
      document.removeEventListener('fullscreenchange', onChange);
    };
  }, []);

  const toggleFull = (): void => {
    const el = fullscreenRef.current;
    if (document.fullscreenElement) void document.exitFullscreen();
    else if (el) void el.requestFullscreen();
  };

  return (
    <div className="map-toolbar" role="toolbar" aria-label="Map toolbar">
      <label className="map-toolbar__field">
        <span>{M.world}</span>
        <Select
          aria-label={M.world}
          value={String(worldId)}
          onChange={(e) => {
            if (e.target.value === 'all') {
              setShowAll((v) => !v);
              return;
            }
            void session.goTo({ world: Number(e.target.value), layer: 0 });
          }}
        >
          {options.map((g) => (
            <optgroup key={g.group} label={g.label}>
              {g.rows.map((w) => (
                <option key={w.id} value={String(w.id)}>
                  {w.name}
                  {w.online > 0 ? ` (${String(w.online)})` : ''}
                </option>
              ))}
            </optgroup>
          ))}
          <option value="all">{showAll ? M.hideEmptyWorlds : M.showAllWorlds}</option>
        </Select>
      </label>
      <label className="map-toolbar__field">
        <span>{M.layer}</span>
        <Select
          aria-label={M.layer}
          value={String(layer)}
          onChange={(e) => {
            void session.setLayer(Number(e.target.value));
          }}
        >
          <option value="0">{M.layerDefault}</option>
          {live.map((l) => (
            <option key={l.layer} value={String(l.layer)}>
              {`${String(l.layer)} · ${l.owner || l.kind} (${String(l.members)})`}
            </option>
          ))}
          {layer !== 0 && !live.some((l) => l.layer === layer) ? (
            <option value={String(layer)}>{`${String(layer)} (closed)`}</option>
          ) : null}
        </Select>
      </label>
      <FindBox session={session} inputRef={findRef} />
      <div className="map-toolbar__group">
        <Button
          size="sm"
          variant="ghost"
          icon="filter"
          onClick={() => {
            session.setTab('layers');
          }}
        >
          {M.layers}
        </Button>
        <Button
          size="sm"
          variant={follow ? 'primary' : 'ghost'}
          icon="pin"
          aria-pressed={follow !== null}
          disabled={follow === null && selection.length === 0}
          title={follow ? M.following(follow.name) : M.follow}
          onClick={() => {
            session.toggleFollow();
          }}
        >
          {follow ? M.following(follow.name) : M.follow}
        </Button>
        <Button
          size="sm"
          variant={tool === 'select' ? 'primary' : 'ghost'}
          icon="sliders"
          aria-pressed={tool === 'select'}
          title={M.selectTitle}
          onClick={() => {
            session.setTool(tool === 'select' ? 'pan' : 'select');
          }}
        >
          {M.select}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="refresh"
          aria-label={M.refresh}
          title={M.refresh}
          onClick={() => {
            session.refresh();
          }}
        />
        <Button
          size="sm"
          variant="ghost"
          icon="monitor"
          aria-label={full ? M.exitFullscreen : M.fullscreen}
          title={full ? M.exitFullscreen : M.fullscreen}
          onClick={toggleFull}
        />
      </div>
    </div>
  );
}

// ---- find ---------------------------------------------------------------------------------------------------------------------------

/** "6960 3274" or "6960, 3274": a point of the current world. */
export function parsePoint(text: string): { x: number; z: number } | null {
  const m = /^\s*(-?\d+(?:\.\d+)?)[\s,;]+(-?\d+(?:\.\d+)?)\s*$/.exec(text);
  if (!m) return null;
  return { x: Number(m[1]), z: Number(m[2]) };
}

const KIND_LABEL: Readonly<Record<string, string>> = {
  player: 'Player',
  monster: 'Monster',
  npc: 'NPC',
  pet: 'Pet',
  region: 'Region',
  location: 'Place',
};

function FindBox({
  session,
  inputRef,
}: {
  readonly session: MapSession;
  readonly inputRef: React.RefObject<HTMLInputElement | null>;
}) {
  const [text, setText] = useState('');
  const [result, setResult] = useState<{
    readonly q: string;
    readonly hits: readonly FindHit[] | null;
  } | null>(null);
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const worlds = useStore(session.store, (s) => s.worlds);

  // the query is the text once it is long enough and is not a point: the answer belongs to the query it was asked for
  const q = text.trim();
  const active = q.length >= 2 && parsePoint(q) === null;
  const current = active && result?.q === q ? result : null;
  const hits = current?.hits ?? null;
  const failed = current !== null && current.hits === null;
  const busy = active && current === null;

  useEffect(() => {
    if (!active) return;
    const t = setTimeout(() => {
      session.find(q).then(
        (h) => {
          setResult({ q, hits: h });
        },
        () => {
          setResult({ q, hits: null });
        },
      );
    }, 250);
    return () => {
      clearTimeout(t);
    };
  }, [active, q, session]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent): void => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => {
      document.removeEventListener('mousedown', onDown);
    };
  }, [open]);

  const choose = (h: FindHit): void => {
    setOpen(false);
    void session.reveal(h);
  };

  const submit = (): void => {
    const p = parsePoint(text);
    if (p) {
      setOpen(false);
      void session.flyTo({
        world: session.ui.worldId,
        layer: session.ui.layer,
        x: p.x,
        z: p.z,
        upp: 1,
      });
      return;
    }
    const first = hits?.[0];
    if (first) choose(first);
  };

  const worldName = (id: number): string =>
    worlds?.worlds.find((w) => w.id === id)?.name ?? `world ${String(id)}`;

  return (
    <div className="map-find" ref={wrap}>
      <Icon name="search" size={14} />
      <input
        ref={inputRef}
        className="map-find__input"
        type="search"
        aria-label={M.find}
        placeholder={M.findPlaceholder}
        value={text}
        onFocus={() => {
          setOpen(true);
        }}
        onChange={(e) => {
          setText(e.target.value);
          setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            submit();
          } else if (e.key === 'Escape') {
            setOpen(false);
            e.currentTarget.blur();
          }
        }}
      />
      {open && text.trim().length >= 2 && (hits !== null || failed || busy) ? (
        <ul className="map-find__results" role="listbox" aria-label="Find results">
          {failed ? <li className="map-find__note">{M.findFailed}</li> : null}
          {!failed && hits !== null && hits.length === 0 && !busy ? (
            <li className="map-find__note">{M.findNothing}</li>
          ) : null}
          {(hits ?? []).map((h, i) => (
            <li
              key={`${h.kind}-${String(h.obj_id)}-${String(i)}`}
              role="option"
              aria-selected="false"
            >
              <button
                type="button"
                className="map-find__hit"
                onClick={() => {
                  choose(h);
                }}
              >
                <span className="map-find__kind">{KIND_LABEL[h.kind] ?? h.kind}</span>
                <span className="map-find__name">{h.name}</span>
                <span className="map-find__where">
                  {worldName(h.world)}
                  {h.layer === 0 ? '' : ` · layer ${String(h.layer)}`}
                  {h.level > 0 && (h.kind === 'player' || h.kind === 'monster')
                    ? ` · Lv ${String(h.level)}`
                    : ''}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
