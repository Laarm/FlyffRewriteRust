// The map's keys in the hash of the console (Plan/25 section 2.3): `#/world/map?w=1&l=0&x=6960&z=3274&zoom=2&sel=p:55`. The map writes them with
// `replace` (no history entry per pan) and keeps every other key of the route (an open drawer, a pinned record).
import { getRoute, navigate } from '../stores/route';
import { MAP_PARAM_KEYS } from '../components/mapPlace';

/** Replaces the map's keys of the current route; a no-op when the route is not the Map tab. */
export function writeMapRoute(params: Readonly<Record<string, string | undefined>>): void {
  const r = getRoute();
  if (r?.category !== 'world' || r.tab !== 'map') return;
  const next: Record<string, string> = { ...r.params };
  for (const k of MAP_PARAM_KEYS) Reflect.deleteProperty(next, k);
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') next[k] = v;
  navigate({ ...r, params: next }, { replace: true });
}
