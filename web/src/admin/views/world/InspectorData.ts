// What the inspector cards read besides the map: the object (`GET /admin/api/objects/{obj}`, every 2 s while a card is open), the character behind a
// player (`GET /admin/api/characters/{id}`, every 10 s, with `view.players`), the session (when the player entered the world, the address with
// `view.pii`), the respawn lines of the world and the acting GM character. Each is polled only while polling is allowed (`app/activity.ts`).
import { useEffect, useMemo, useState } from 'react';
import type { EntityInfo, MapApi } from '../../world/api';
import { isApiError } from '../../api/rest';
import type { CharacterProfile, ObjectDetail, SpawnRowDto } from '../../gen/dto';
import { actingFor, useActing } from '../../stores/acting';
import { useSession } from '../../stores/session';
import type { ActingCharacter } from '../../gen/dto';
import { useSnapshot } from '../dashboard/useSnapshot';
import {
  fetchObject,
  fetchProfile,
  fetchSessions,
  fetchSpawnTable,
  type SessionRow,
} from './InspectorApi';

export interface ObjectState {
  /** The latest card; kept while a later poll fails. */
  readonly detail: ObjectDetail | null;
  /** The server says the object does not exist (any more). */
  readonly gone: boolean;
  readonly error: unknown;
}

/** The card of one object, refreshed every `everyMs`; `null` reads nothing. A change of object forgets the old card at once. */
export function useObjectDetail(obj: number | null, everyMs = 2000): ObjectState {
  const snap = useSnapshot<ObjectDetail | null>(
    async (signal) => (obj === null ? null : await fetchObject(obj, signal)),
    `object:${String(obj)}`,
    everyMs,
  );
  const gone = isApiError(snap.error, 'not_found');
  // a card of another object must not show for this one (the snapshot already drops answers of an older key)
  const detail = snap.data !== null && snap.data.obj_id === obj ? snap.data : null;
  return { detail, gone, error: snap.error };
}

export interface PlayerExtra {
  readonly profile: CharacterProfile | null;
  readonly session: SessionRow | null;
}

/** The character behind a player card: the account and the party (`view.players`), the session (entered the world, address). */
export function usePlayerExtra(characterId: number, enabled: boolean): PlayerExtra {
  const profile = useSnapshot<CharacterProfile | null>(
    async (signal) => (enabled && characterId > 0 ? await fetchProfile(characterId, signal) : null),
    `profile:${String(characterId)}:${String(enabled)}`,
    10_000,
  );
  const sessions = useSnapshot<readonly SessionRow[]>(
    async (signal) => (enabled && characterId > 0 ? await fetchSessions(signal) : []),
    `sessions:${String(characterId)}:${String(enabled)}`,
    15_000,
  );
  const data = profile.data;
  const session = useMemo(
    () => sessions.data?.find((s) => s.character_id === characterId) ?? null,
    [sessions.data, characterId],
  );
  return {
    profile: data !== null && data.character.id === characterId ? data : null,
    session,
  };
}

const spawnCache = new Map<number, { at: number; lines: readonly SpawnRowDto[] }>();

/** The respawn lines of a world (ten seconds old at most; the rectangle is what the cards and the tools need). */
export async function spawnLines(world: number, now = Date.now()): Promise<readonly SpawnRowDto[]> {
  const hit = spawnCache.get(world);
  if (hit && now - hit.at < 10_000) return hit.lines;
  const table = await fetchSpawnTable(world);
  spawnCache.set(world, { at: now, lines: table.lines });
  return table.lines;
}

/** Tests: forgets the cached respawn lines. */
export function resetSpawnCacheForTests(): void {
  spawnCache.clear();
}

/** The line a monster was spawned by (`ObjectDetail.spawn_region` is the line plus one), or `null`. */
export function useSpawnLine(world: number, spawnRegion: number): SpawnRowDto | null {
  const key = `${String(world)}/${String(spawnRegion)}`;
  const [found, setFound] = useState<{ key: string; line: SpawnRowDto | null } | null>(null);
  useEffect(() => {
    if (spawnRegion <= 0) return;
    let live = true;
    spawnLines(world).then(
      (lines) => {
        if (live) setFound({ key, line: lines.find((l) => l.region === spawnRegion - 1) ?? null });
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [world, spawnRegion, key]);
  return spawnRegion > 0 && found?.key === key ? found.line : null;
}

/** The acting GM character of this session (Plan/25 section 1.5): the staff member's own in-world character, or an owner's pick. */
export function useActingCharacter(): ActingCharacter | null {
  const info = useSession((s) => s.info);
  const chosen = useActing((s) => s.chosen);
  const candidates = useActing((s) => s.candidates);
  return useMemo(() => actingFor(info, chosen, candidates), [info, chosen, candidates]);
}

/** Re-renders its component every `ms` milliseconds (0 = never): the map's store holds the frames outside React, a card reads them again each beat. */
export function useTick(ms: number): void {
  const [, set] = useState(0);
  useEffect(() => {
    if (ms <= 0) return;
    const t = setInterval(() => {
      set((n) => n + 1);
    }, ms);
    return () => {
      clearInterval(t);
    };
  }, [ms]);
}

/** The frames' snapshot of an entity, read again every second (the store changes a few times a second at most, never per frame). */
export function useEntity(api: MapApi, id: number | null): EntityInfo | null {
  useTick(1000);
  return id === null ? null : api.entity(id);
}

/** The snapshots of several entities (the ones in view), read again every second. */
export function useEntitiesOf(api: MapApi, ids: readonly number[]): EntityInfo[] {
  useTick(1000);
  return ids.map((id) => api.entity(id)).filter((e): e is EntityInfo => e !== null);
}
