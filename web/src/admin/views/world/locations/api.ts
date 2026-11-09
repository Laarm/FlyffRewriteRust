// The calls of the quick locations (Plan/25 section 11.3): `GET /admin/api/locations` and the bookmark routes (`act.world`).
import { mutation, rest } from '../../../api/rest';
import type { Reason } from '../../../gen/dto';
import type { BookmarkBody, LocationRow, LocationsDoc } from '../../../gen/dto';

export type {
  BookmarkBody,
  GroupDoc,
  LocationKind,
  LocationPolicy,
  LocationRow,
  LocationsDoc,
} from '../../../gen/dto';

/** `GET /admin/api/locations`: the default buttons resolved from the game data, the world cameras and the shared bookmarks. */
export function fetchLocations(signal?: AbortSignal): Promise<LocationsDoc> {
  return rest.get<LocationsDoc>('/admin/api/locations', signal ? { signal } : {});
}

export function createBookmark(body: BookmarkBody, reason?: Reason): Promise<LocationRow> {
  return rest.post<LocationRow>('/admin/api/locations', mutation(body, reason));
}

export function updateBookmark(
  id: number,
  body: BookmarkBody,
  reason?: Reason,
): Promise<LocationRow> {
  return rest.put<LocationRow>(`/admin/api/locations/${String(id)}`, mutation(body, reason));
}

export function deleteBookmark(id: number, reason?: Reason): Promise<{ id: number }> {
  return rest.delete<{ id: number }>(`/admin/api/locations/${String(id)}`, mutation({}, reason));
}

/** The body of the bookmark route for a row (to edit a copy of a default, or to save a changed bookmark). */
export function bodyOf(row: LocationRow): BookmarkBody {
  return {
    group: row.group,
    name: row.name,
    world: row.world,
    layer_policy: row.layer_policy,
    x: row.x,
    z: row.z,
    ...(row.own_zoom !== null
      ? { zoom: row.own_zoom }
      : row.kind === 'bookmark'
        ? {}
        : row.zoom !== null
          ? { zoom: row.zoom }
          : {}),
    sort: row.sort,
  };
}
