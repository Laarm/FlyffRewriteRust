// The REST calls of the World & Content tabs (Plan/25 section 7, 11.3). The rates and the event flags keep the routes of Plan/10 section 13.2
// (plain bodies, no envelope); everything newer uses the `Mutation` envelope.
import { mutation, rest } from '../../api/rest';
import type {
  ActionResult,
  EventInfoDto,
  MonsterCensusDto,
  Reason,
  SpawnTableDto,
  WeatherKindDto,
  WeatherResult,
} from '../../gen/dto';
import type { EventRow } from './events';
import type { RateRow } from './rates';

/** `GET /admin/api/rates`. */
export function listRates(signal?: AbortSignal): Promise<RateRow[]> {
  return rest.get<RateRow[]>('/admin/api/rates', signal ? { signal } : {});
}

/** `PUT /admin/api/rates/{key}`: `null` goes back to the value of the file. */
export function setRate(key: string, value: number | null): Promise<RateRow> {
  return rest.put<RateRow>(`/admin/api/rates/${encodeURIComponent(key)}`, { value });
}

/** `GET /admin/api/events`. */
export function listEvents(signal?: AbortSignal): Promise<EventRow[]> {
  return rest.get<EventRow[]>('/admin/api/events', signal ? { signal } : {});
}

/** `PUT /admin/api/events/{id}`. */
export function setEvent(id: number, state: number): Promise<EventRow> {
  return rest.put<EventRow>(`/admin/api/events/${String(id)}`, { state });
}

/** `GET /admin/api/events/modifiers`. */
export function eventInfo(signal?: AbortSignal): Promise<EventInfoDto> {
  return rest.get<EventInfoDto>('/admin/api/events/modifiers', signal ? { signal } : {});
}

/** `PUT /admin/api/weather/{world}`. */
export function setWeather(
  world: number,
  kind: WeatherKindDto,
  reason?: Reason,
): Promise<WeatherResult> {
  return rest.put<WeatherResult>(`/admin/api/weather/${String(world)}`, mutation({ kind }, reason));
}

/** `GET /admin/api/spawns?world=`. */
export function spawnTable(world: number, signal?: AbortSignal): Promise<SpawnTableDto> {
  return rest.get<SpawnTableDto>('/admin/api/spawns', {
    query: { world },
    ...(signal ? { signal } : {}),
  });
}

/** `GET /admin/api/census?world=`. */
export function monsterCensus(world: number, signal?: AbortSignal): Promise<MonsterCensusDto> {
  return rest.get<MonsterCensusDto>('/admin/api/census', {
    query: { world },
    ...(signal ? { signal } : {}),
  });
}

/** `POST /admin/api/spawns/{region}/respawn`. */
export function respawnNow(
  region: number,
  world: number,
  layer: number,
  reason?: Reason,
): Promise<ActionResult> {
  return rest.post<ActionResult>(
    `/admin/api/spawns/${String(region)}/respawn`,
    mutation({ world, layer }, reason),
  );
}
