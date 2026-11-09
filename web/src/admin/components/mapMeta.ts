// The world documents of the admin API (`GET /admin/api/worlds`, `GET /admin/api/world/{id}/meta`), cached for the life of the tab and shared by
// the live map and the static previews. A document is static for the life of the server (the data does not change), only the online counts of
// the world list do: `loadWorlds({ fresh: true })` asks again. Lives in `components/` so that a preview does not import the map engine.
import { rest } from '../api/rest';
import type { MetaDoc, WorldsDoc } from '../world/metaTypes';

const metas = new Map<number, Promise<MetaDoc>>();
let worlds: Promise<WorldsDoc> | null = null;

/** The world list (with the online counts); `fresh` bypasses the cache (the selector refreshes the counts). */
export function loadWorlds(options: { readonly fresh?: boolean } = {}): Promise<WorldsDoc> {
  if (options.fresh === true || worlds === null) {
    const p = rest.get<WorldsDoc>('/admin/api/worlds');
    worlds = p;
    // a failed load is forgotten so that the next call asks again
    p.catch(() => {
      if (worlds === p) worlds = null;
    });
  }
  return worlds;
}

/** The meta document of one world (cached; a failure is forgotten). */
export function loadMeta(id: number): Promise<MetaDoc> {
  let p = metas.get(id);
  if (!p) {
    const q = rest.get<MetaDoc>(`/admin/api/world/${String(id)}/meta`);
    p = q;
    metas.set(id, q);
    q.catch(() => {
      if (metas.get(id) === q) metas.delete(id);
    });
  }
  return p;
}

/** Tests: forgets everything. */
export function resetMapMeta(): void {
  metas.clear();
  worlds = null;
}
