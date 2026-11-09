// The Locations panel at the left of the map (Plan/25 section 3.2, 3.13). The quick locations of WP-ADMIN-14 (eight sources, shared bookmarks,
// favourites, layer popovers) replace this through the `locations` slot (`views/world/slots.tsx`); until then the panel lists what the world's own
// meta document knows: its teleport and revival points and its portals. 200 px wide, or a 44 px rail with the first letters.
import { useStore } from 'zustand';
import { Button } from '../components/Button';
import type { MapSession } from './session';

export interface LocationsPanelProps {
  readonly session: MapSession;
  readonly rail: boolean;
  readonly onToggleRail: () => void;
}

/** Two letters for the rail: the tag in front of a name (`[Town] Flaris`) is dropped. */
export function initials(name: string): string {
  const words = name
    .replace(/^\s*\[[^\]]*\]\s*/, '')
    .split(/\s+/)
    .filter(Boolean);
  return (
    words.length > 1
      ? (words[0]?.[0] ?? '') + (words[1]?.[0] ?? '')
      : (words[0] ?? name).slice(0, 2)
  ).toUpperCase();
}

export function LocationsPanel({ session, rail, onToggleRail }: LocationsPanelProps) {
  const meta = useStore(session.store, (s) => s.meta);
  const places: { name: string; x: number; z: number; group: string }[] = [];
  if (meta) {
    for (const t of meta.teleports)
      places.push({ name: t.name, x: t.pos[0], z: t.pos[2], group: 'Towns' });
    for (const r of meta.revival)
      places.push({ name: r.title ?? r.key, x: r.pos[0], z: r.pos[2], group: 'Revival' });
  }
  return (
    <div className="map-locations" data-rail={rail ? 'true' : 'false'}>
      <div className="map-locations__head">
        {rail ? null : <h3>Locations</h3>}
        <Button
          size="sm"
          variant="ghost"
          aria-label={rail ? 'Expand the locations panel' : 'Collapse the locations panel'}
          title={rail ? 'Expand' : 'Collapse'}
          onClick={onToggleRail}
        >
          {rail ? '»' : '«'}
        </Button>
      </div>
      <ul className="map-locations__list">
        {places.map((p) => (
          <li key={`${p.group}-${p.name}`}>
            <button
              type="button"
              className="map-locations__item"
              title={p.name}
              onClick={() => {
                if (meta) void session.flyTo({ world: meta.id, layer: 0, x: p.x, z: p.z, upp: 2 });
              }}
            >
              {rail ? initials(p.name) : p.name}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
