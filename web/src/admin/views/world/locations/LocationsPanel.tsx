// The compact quick-locations panel at the left of the map (Plan/25 section 3.2, 3.13): "Jump to me", the favourites (keys 1 to 9, drag to reorder),
// the ten most recent jumps, and the groups folded to one line ("Dungeons ▸ 4", "Instances ▸ 5 (3 live)") that open to their buttons. 200 px wide, or a
// 44 px rail with the favourites as two-letter buttons. Everything on it comes from `GET /admin/api/locations`; this file holds no coordinate.
// A click flies the map there (the layer by the policy of the row, a popover when several live layers exist), a right click or a long press opens
// the actions of the button, a hover on a teleporter shows its picture.
import { useEffect, useMemo, useRef, useState } from 'react';
import { Button, Skeleton } from '../../../components';
import { safeStorage } from '../../../../platform/storage';
import type { MapSlotProps } from '../../../world/api';
import { useActingCharacter } from '../InspectorData';
import { useActionEnv } from '../InspectorActions';
import { jumpToMe } from '../InspectorSummary';
import { ContextMenu } from '../ContextMenu';
import { layersOfWorld, useLayerFeed, useLayers } from '../layers';
import { useAssetUrl } from '../../players/gamedata';
import type { GroupDoc, LocationRow } from './api';
import { openBookmarkDialog } from './BookmarkDialog';
import { jumpToLocation } from './jump';
import { locationActions } from './LocationMenu';
import { L } from './strings';
import { OPEN_KEY, rowById, useLocations, useLocationsFeed } from './store';
import './locations.css';

const LONG_PRESS_MS = 550;

/** Two letters for the rail: the tag in front of a name is dropped. */
export function initials(name: string): string {
  const words = name
    .replace(/^\s*\[[^\]]*\]\s*/, '')
    .split(/\s+/)
    .filter(Boolean);
  const first = words[0] ?? name;
  return (
    words.length > 1 ? (first[0] ?? '') + (words[1]?.[0] ?? '') : first.slice(0, 2)
  ).toUpperCase();
}

function readOpen(): ReadonlySet<string> | null {
  try {
    const raw = safeStorage.local.get(OPEN_KEY);
    if (raw === null) return null;
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v) ? new Set(v.filter((e): e is string => typeof e === 'string')) : null;
  } catch {
    return null;
  }
}

/** The live layers of the worlds a set of rows points into (a `pick_live` row only). */
export function liveCount(
  rows: readonly LocationRow[],
  layers: readonly { world: number; layer: number }[],
): number {
  const worlds = new Set(rows.filter((r) => r.layer_policy === 'pick_live').map((r) => r.world));
  let n = 0;
  for (const w of worlds)
    n += layersOfWorld(
      layers.map((l) => ({
        ...l,
        kind: '',
        owner: '',
        members: 0,
        monsters: 0,
        secondsLeft: 0,
        createdTick: 0,
      })),
      w,
    ).length;
  return n;
}

function Thumb({
  row,
  top,
  left,
}: {
  readonly row: LocationRow;
  readonly top: number;
  readonly left: number;
}) {
  const url = useAssetUrl(row.thumb ? `theme/${row.thumb}` : '');
  if (!row.thumb || !url) return null;
  return (
    <div className="loc-thumb" style={{ top, left }} data-testid="location-thumb">
      <img src={url} alt="" width={160} />
      <div className="loc-thumb__cap">
        {row.min_level !== null && row.max_level !== null
          ? L.panel.levels(row.min_level, row.max_level)
          : row.name}
      </div>
    </div>
  );
}

interface HoverRow {
  readonly row: LocationRow;
  readonly top: number;
  readonly left: number;
}

export default function LocationsPanel({
  api,
  rail,
  onToggleRail,
}: MapSlotProps & { readonly rail: boolean; readonly onToggleRail: () => void }) {
  useLocationsFeed();
  useLayerFeed();
  const env = useActionEnv(api);
  const acting = useActingCharacter();
  const doc = useLocations((s) => s.doc);
  const status = useLocations((s) => s.status);
  const error = useLocations((s) => s.error);
  const favourites = useLocations((s) => s.favourites);
  const recent = useLocations((s) => s.recent);
  const layers = useLayers((s) => s.layers);
  const [open, setOpen] = useState<ReadonlySet<string> | null>(() => readOpen());
  const [menu, setMenu] = useState<{ x: number; y: number; row: LocationRow } | null>(null);
  const [hover, setHover] = useState<HoverRow | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const press = useRef<{ timer: ReturnType<typeof setTimeout>; fired: boolean } | null>(null);

  // the favourite keys 1 to 9 of the map
  useEffect(
    () =>
      api.onFavourite((index) => {
        const st = useLocations.getState();
        const row = rowById(st.doc, st.favourites[index] ?? '');
        if (row) void jumpToLocation(api, row);
      }),
    [api],
  );

  const groups = doc?.groups ?? [];
  const isOpen = (g: GroupDoc, i: number): boolean => (open === null ? i === 0 : open.has(g.id));
  const toggle = (g: GroupDoc, i: number): void => {
    const now = new Set(open ?? (groups[0] ? [groups[0].id] : []));
    if (isOpen(g, i)) now.delete(g.id);
    else now.add(g.id);
    safeStorage.local.set(OPEN_KEY, JSON.stringify([...now]));
    setOpen(now);
  };
  const byGroup = useMemo(() => {
    const m = new Map<string, LocationRow[]>();
    for (const r of doc?.locations ?? []) {
      const key = r.group.toLowerCase();
      const list = m.get(key);
      if (list) list.push(r);
      else m.set(key, [r]);
    }
    return m;
  }, [doc]);
  const favRows = favourites
    .map((id) => rowById(doc, id))
    .filter((r): r is LocationRow => r !== undefined);
  const recentRows = recent
    .map((id) => rowById(doc, id))
    .filter((r): r is LocationRow => r !== undefined);

  const rowButton = (
    row: LocationRow,
    extra?: { index?: number; draggable?: boolean },
  ): React.ReactNode => (
    <button
      type="button"
      className={`loc-item${rail ? ' loc-item--rail' : ''}`}
      data-location={row.id}
      title={row.name}
      {...(rail ? { 'aria-label': row.name } : {})}
      draggable={extra?.draggable === true}
      onClick={() => {
        if (press.current?.fired === true) {
          press.current = null;
          return;
        }
        void jumpToLocation(api, row);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY, row });
      }}
      onPointerDown={(e) => {
        if (e.pointerType === 'mouse') return;
        const x = e.clientX;
        const y = e.clientY;
        press.current = {
          fired: false,
          timer: setTimeout(() => {
            if (press.current) press.current.fired = true;
            setMenu({ x, y, row });
          }, LONG_PRESS_MS),
        };
      }}
      onPointerUp={() => {
        if (press.current) clearTimeout(press.current.timer);
      }}
      onPointerLeave={() => {
        if (press.current) clearTimeout(press.current.timer);
        setHover(null);
      }}
      onPointerEnter={(e) => {
        if (row.thumb) {
          const r = e.currentTarget.getBoundingClientRect();
          setHover({ row, top: r.top, left: r.right + 8 });
        }
      }}
      onDragStart={(e) => {
        e.dataTransfer.setData('text/plain', row.id);
        setDragging(row.id);
      }}
      onDragOver={(e) => {
        if (dragging !== null) e.preventDefault();
      }}
      onDrop={(e) => {
        e.preventDefault();
        if (dragging !== null && extra?.index !== undefined)
          useLocations.getState().moveFavourite(dragging, extra.index);
        setDragging(null);
      }}
      onDragEnd={() => {
        setDragging(null);
      }}
    >
      {extra?.index !== undefined && !rail ? (
        <span className="loc-item__key">{extra.index + 1}</span>
      ) : null}
      <span className="loc-item__name">{rail ? initials(row.name) : row.name}</span>
      {row.kind === 'bookmark' && !rail ? (
        <span className="loc-item__mark" aria-hidden="true">
          ●
        </span>
      ) : null}
    </button>
  );

  const add = (
    <Button
      size="sm"
      variant="ghost"
      title={L.panel.addBookmarkHint}
      aria-label={L.panel.addBookmark}
      onClick={() => {
        const s = api.store.getState();
        openBookmarkDialog({ world: s.worldId, x: s.view.cx, z: s.view.cz });
      }}
    >
      {rail ? '+' : L.panel.addBookmark}
    </Button>
  );
  const me = (
    <Button
      size="sm"
      variant="secondary"
      data-testid="jump-to-me"
      aria-label={L.panel.jumpToMe}
      title={acting ? L.panel.jumpToMeHint : L.panel.jumpOffline}
      disabled={acting === null}
      onClick={() => {
        jumpToMe(api);
      }}
    >
      {rail ? '⌖' : `⌖ ${L.panel.jumpToMe}`}
    </Button>
  );

  return (
    <div
      className="map-locations loc"
      data-testid="locations-panel"
      data-rail={rail ? 'true' : 'false'}
      role="region"
      aria-label={L.panel.region}
    >
      <div className="map-locations__head">
        {rail ? null : <h3>{L.panel.title}</h3>}
        <Button
          size="sm"
          variant="ghost"
          aria-label={rail ? L.panel.expand : L.panel.collapse}
          title={rail ? L.panel.expand : L.panel.collapse}
          onClick={onToggleRail}
        >
          {rail ? '»' : '«'}
        </Button>
      </div>
      <div className="loc__body">
        {me}
        {status === 'loading' && doc === null ? <Skeleton lines={5} /> : null}
        {status === 'error' && doc === null ? (
          <div className="loc__error" role="alert">
            <p>{L.panel.failed}</p>
            {rail ? null : <p className="adm-muted">{error}</p>}
            <Button
              size="sm"
              onClick={() => {
                void useLocations.getState().load(true);
              }}
            >
              {L.panel.retry}
            </Button>
          </div>
        ) : null}
        {doc ? (
          <>
            {rail ? null : <h4 className="loc__h">{`★ ${L.panel.favourites}`}</h4>}
            {favRows.length === 0 ? (
              rail ? null : (
                <p className="loc__hint">{L.panel.favouritesHint}</p>
              )
            ) : (
              <ul className="loc__list" data-testid="favourites">
                {favRows.map((r, i) => (
                  <li key={r.id}>{rowButton(r, { index: i, draggable: true })}</li>
                ))}
              </ul>
            )}
            {rail ? null : (
              <>
                {recentRows.length > 0 ? (
                  <details className="loc__group" data-testid="recent">
                    <summary>{L.panel.group(L.panel.recent, recentRows.length)}</summary>
                    <ul className="loc__list">
                      {recentRows.map((r) => (
                        <li key={r.id}>{rowButton(r)}</li>
                      ))}
                    </ul>
                  </details>
                ) : null}
                {groups.map((g, i) => {
                  const rows = byGroup.get(g.label.toLowerCase()) ?? [];
                  const live = liveCount(rows, layers);
                  const expanded = isOpen(g, i);
                  return (
                    <div key={g.id} className="loc__group" data-group={g.id}>
                      <button
                        type="button"
                        className="loc__group-head"
                        aria-expanded={expanded}
                        title={
                          expanded ? L.panel.collapseGroup(g.label) : L.panel.expandGroup(g.label)
                        }
                        onClick={() => {
                          toggle(g, i);
                        }}
                      >
                        <span>{expanded ? '▾' : '▸'}</span>
                        <span className="loc__group-name">{g.label}</span>
                        <span className="loc__count">{g.count}</span>
                        {live > 0 ? (
                          <span
                            className="loc__live"
                            data-testid="live-count"
                          >{`${String(live)} live`}</span>
                        ) : null}
                      </button>
                      {expanded ? (
                        <ul className="loc__list">
                          {rows.map((r) => (
                            <li key={r.id}>{rowButton(r)}</li>
                          ))}
                        </ul>
                      ) : null}
                    </div>
                  );
                })}
                {doc.unresolved.length > 0 ? (
                  <p className="loc__hint">{L.panel.unresolved(doc.unresolved.length)}</p>
                ) : null}
              </>
            )}
            {env.caps.has('act.world') ? add : null}
          </>
        ) : null}
      </div>
      {hover ? <Thumb row={hover.row} top={hover.top} left={hover.left} /> : null}
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          title={menu.row.name}
          actions={locationActions(env, api, menu.row)}
          onClose={() => {
            setMenu(null);
          }}
          label={`${L.panel.region}: ${menu.row.name}`}
        />
      ) : null}
    </div>
  );
}
