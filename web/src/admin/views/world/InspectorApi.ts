// The calls of the map's inspector and tools (Plan/25 section 3.12, 11.3): `POST /admin/api/actions/{action}` (a star route: every call carries an
// `Idempotency-Key`), `GET /admin/api/objects/{obj}` and the reads a card shows next to it. One typed function per action; the caller (the card, the
// context menu, the tools) collects the reason and the typed confirmation with `runConfirmed` and passes them as `given`.
import { mutation, rest } from '../../api/rest';
import type { ConfirmResult } from '../../components/ConfirmDialog';
import type {
  ActionResult,
  AreaKind,
  CharacterProfile,
  ObjectDetail,
  SpawnTableDto,
} from '../../gen/dto';

const envelope = <T>(body: T, given: ConfirmResult) => mutation(body, given.reason, given.confirm);

function action(name: string, body: unknown, given: ConfirmResult = {}): Promise<ActionResult> {
  return rest.post<ActionResult>(`/admin/api/actions/${name}`, envelope(body, given), {
    idempotent: true,
  });
}

/** Where the map showed the objects: the server answers `stale_target` for one that is elsewhere now. */
export interface Where {
  readonly world: number;
  readonly layer: number;
}

// ---- reads --------------------------------------------------------------------------------------------------------------------

/** `GET /admin/api/objects/{obj}`: the card of an object (`404` when it is gone). */
export function fetchObject(obj: number, signal?: AbortSignal): Promise<ObjectDetail> {
  return rest.get<ObjectDetail>(`/admin/api/objects/${String(obj)}`, signal ? { signal } : {});
}

/** `GET /admin/api/characters/{id}`: the account, the party and the mute of a player (`view.players`). */
export function fetchProfile(id: number, signal?: AbortSignal): Promise<CharacterProfile> {
  return rest.get<CharacterProfile>(
    `/admin/api/characters/${String(id)}`,
    signal ? { signal } : {},
  );
}

/** One row of `GET /admin/api/sessions`. */
export interface SessionRow {
  readonly character_id: number;
  readonly in_world_since: number;
  readonly lingering: boolean;
  /** Only with `view.pii`. */
  readonly ip?: string;
}

/** `GET /admin/api/sessions`: when the players entered the world and (with `view.pii`) from where. */
export async function fetchSessions(signal?: AbortSignal): Promise<readonly SessionRow[]> {
  const body = await rest.get<{ in_world: readonly SessionRow[] }>(
    '/admin/api/sessions',
    signal ? { signal } : {},
  );
  return body.in_world;
}

/** `GET /admin/api/spawns?world=`: the respawn lines of a world (their rectangles and counters). */
export function fetchSpawnTable(world: number, signal?: AbortSignal): Promise<SpawnTableDto> {
  return rest.get<SpawnTableDto>('/admin/api/spawns', {
    query: { world },
    ...(signal ? { signal } : {}),
  });
}

// ---- actions ------------------------------------------------------------------------------------------------------------------

export function teleport(
  characters: readonly number[],
  to: Where & { readonly pos: readonly [number, number, number] },
  label: 'point' | 'location',
  given: ConfirmResult,
): Promise<ActionResult> {
  return action(
    'teleport',
    { characters, world: to.world, layer: to.layer, pos: to.pos, label },
    given,
  );
}

/** `summon` / `goto` run `/su` and `/te` as the acting GM character (`as_character` names it when the session has several to choose from). */
export function summon(character: number, asCharacter: number | null): Promise<ActionResult> {
  return action('summon', { character, ...(asCharacter ? { as_character: asCharacter } : {}) });
}

export function goToCharacter(
  character: number,
  asCharacter: number | null,
): Promise<ActionResult> {
  return action('goto', { character, ...(asCharacter ? { as_character: asCharacter } : {}) });
}

export function goToPoint(
  to: Where & { readonly x: number; readonly z: number },
  asCharacter: number | null,
): Promise<ActionResult> {
  return action('goto', {
    world: to.world,
    pos: [Math.max(1, Math.round(to.x)), 0, Math.max(1, Math.round(to.z))],
    ...(asCharacter ? { as_character: asCharacter } : {}),
  });
}

export function heal(
  objs: readonly number[],
  at: Where,
  given: ConfirmResult = {},
): Promise<ActionResult> {
  return action('heal', { objs, world: at.world, layer: at.layer }, given);
}

export function kill(
  objs: readonly number[],
  drops: boolean,
  at: Where,
  given: ConfirmResult,
): Promise<ActionResult> {
  return action('kill', { objs, drops, world: at.world, layer: at.layer }, given);
}

export function despawn(
  objs: readonly number[],
  at: Where,
  given: ConfirmResult,
): Promise<ActionResult> {
  return action('despawn', { objs, world: at.world, layer: at.layer }, given);
}

export function despawnArea(
  at: Where,
  rect: readonly [number, number, number, number],
  kinds: readonly AreaKind[],
  given: ConfirmResult,
): Promise<ActionResult> {
  return action('despawn-area', { world: at.world, layer: at.layer, rect, kinds }, given);
}

export interface SpawnRequest extends Where {
  readonly x: number;
  readonly z: number;
  readonly prop: number;
  readonly count: number;
  readonly spread: number;
  readonly aggressive: boolean;
  /** Seconds; `null` = the monsters stay. */
  readonly lifetimeS: number | null;
}

export function spawn(r: SpawnRequest, given: ConfirmResult = {}): Promise<ActionResult> {
  return action(
    'spawn',
    {
      world: r.world,
      layer: r.layer,
      pos: [r.x, 0, r.z],
      prop: r.prop,
      count: r.count,
      spread: r.spread,
      aggressive: r.aggressive,
      ...(r.lifetimeS === null ? {} : { lifetime_s: r.lifetimeS }),
    },
    given,
  );
}

/** `respawn-region`: makes the respawn line due now (`region` is the line number of `GET /admin/api/spawns`). */
export function respawnRegion(
  at: Where,
  region: number,
  given: ConfirmResult = {},
): Promise<ActionResult> {
  return action('respawn-region', { world: at.world, layer: at.layer, region }, given);
}

// ---- the answer of a group action ------------------------------------------------------------------------------------------------

/** The most frequent reason among the skipped targets of an answer ("stale_target"), or `null` when every target was handled. */
export function skippedBecause(result: ActionResult): string | null {
  const counts = new Map<string, number>();
  for (const r of result.results) {
    if (!r.ok && r.error) counts.set(r.error.code, (counts.get(r.error.code) ?? 0) + 1);
  }
  let best: [string, number] | null = null;
  for (const entry of counts) if (best === null || entry[1] > best[1]) best = entry;
  return best?.[0] ?? null;
}
