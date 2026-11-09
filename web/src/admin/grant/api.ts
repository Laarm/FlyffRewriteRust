// The typed calls of the items area (Plan/25 section 11.3). Every call goes through the admin REST client (`api/rest.ts`): the CSRF header,
// the session, the error envelope and the idempotency header are its business; this file only knows the routes and their shapes.
import { mutation, rest } from '../api/rest';
import type {
  CancelResult,
  CatalogMeta,
  CharacterListItem,
  DuplicatesDto,
  EconomyDto,
  GrantReport,
  GrantRequest,
  GrantRowDto,
  ItemSearchBody,
  ItemSpec,
  Page,
  Reason,
  RemovalRowDto,
  SearchEvent,
  TemplateBody,
  TemplateKindDto,
  TemplateRowDto,
  ValidatedItem,
} from '../gen/dto';

/** `GET /admin/api/items/catalog-meta`. */
export function fetchCatalogMeta(): Promise<CatalogMeta> {
  return rest.get<CatalogMeta>('/admin/api/items/catalog-meta');
}

/** `POST /admin/api/items/validate`: always 200, the problems are inside. */
export function validateItem(spec: ItemSpec, signal?: AbortSignal): Promise<ValidatedItem> {
  return rest.post<ValidatedItem>('/admin/api/items/validate', spec, signal ? { signal } : {});
}

/** `POST /admin/api/grants/v2` ✱: the same `key` repeats the same grant (a retry after a timeout grants nothing more). */
export function postGrant(
  body: GrantRequest,
  reason: Reason,
  confirm: string | undefined,
  key: string,
): Promise<GrantReport> {
  return rest.post<GrantReport>('/admin/api/grants/v2', mutation(body, reason, confirm), {
    idempotent: true,
    idempotencyKey: key,
  });
}

/** `GET /admin/api/characters?q=`: the type-ahead of the recipient field. */
export function searchCharacters(
  q: string,
  limit = 12,
  signal?: AbortSignal,
): Promise<Page<CharacterListItem>> {
  return rest.get<Page<CharacterListItem>>('/admin/api/characters', {
    query: { q, limit },
    ...(signal ? { signal } : {}),
  });
}

/** `GET /admin/api/templates?kind=`. */
export function listTemplates(kind?: TemplateKindDto): Promise<TemplateRowDto[]> {
  return rest.get<TemplateRowDto[]>('/admin/api/templates', kind ? { query: { kind } } : {});
}

export function createTemplate(body: TemplateBody): Promise<TemplateRowDto> {
  return rest.post<TemplateRowDto>('/admin/api/templates', mutation(body));
}

export function updateTemplate(id: number, body: TemplateBody): Promise<TemplateRowDto> {
  return rest.put<TemplateRowDto>(`/admin/api/templates/${String(id)}`, mutation(body));
}

export function deleteTemplate(id: number): Promise<{ id: number }> {
  return rest.delete<{ id: number }>(`/admin/api/templates/${String(id)}`, mutation({}));
}

export type PendingState = 'open' | 'done' | 'cancelled' | 'all';

export interface PendingQuery {
  readonly state: PendingState;
  readonly character?: number;
  readonly days?: number;
}

/** `GET /admin/api/grants`. */
export function fetchGrants(
  q: PendingQuery,
  cursor: string | null,
  signal?: AbortSignal,
): Promise<Page<GrantRowDto>> {
  return rest.get<Page<GrantRowDto>>('/admin/api/grants', {
    query: {
      state: q.state,
      character: q.character,
      days: q.days,
      cursor: cursor ?? undefined,
      limit: 100,
    },
    ...(signal ? { signal } : {}),
  });
}

/** `GET /admin/api/removals`. */
export function fetchRemovals(
  q: PendingQuery,
  cursor: string | null,
  signal?: AbortSignal,
): Promise<Page<RemovalRowDto>> {
  return rest.get<Page<RemovalRowDto>>('/admin/api/removals', {
    query: {
      state: q.state,
      character: q.character,
      days: q.days,
      cursor: cursor ?? undefined,
      limit: 100,
    },
    ...(signal ? { signal } : {}),
  });
}

/** `POST /admin/api/grants/{id}/cancel` or `.../removals/{id}/cancel`. */
export function cancelPending(
  kind: 'grant' | 'removal',
  id: number,
  reason?: Reason,
): Promise<CancelResult> {
  const path = kind === 'grant' ? 'grants' : 'removals';
  return rest.post<CancelResult>(`/admin/api/${path}/${String(id)}/cancel`, mutation({}, reason));
}

/** `POST /admin/api/items/duplicates`. */
export function fetchDuplicates(signal?: AbortSignal): Promise<DuplicatesDto> {
  return rest.post<DuplicatesDto>('/admin/api/items/duplicates', {}, signal ? { signal } : {});
}

/** `GET /admin/api/economy`. */
export function fetchEconomy(
  item: number | null,
  refresh: boolean,
  signal?: AbortSignal,
): Promise<EconomyDto> {
  return rest.get<EconomyDto>('/admin/api/economy', {
    query: { item: item ?? undefined, refresh: refresh ? 1 : undefined },
    ...(signal ? { signal } : {}),
  });
}

/** A line reader over an NDJSON body: calls `onLine` with every complete line, in order; the last line needs no newline. */
export async function readLines(
  body: ReadableStream<Uint8Array>,
  onLine: (line: string) => void,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let pending = '';
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    pending += decoder.decode(value, { stream: true });
    for (;;) {
      const at = pending.indexOf('\n');
      if (at < 0) break;
      const line = pending.slice(0, at).trim();
      pending = pending.slice(at + 1);
      if (line !== '') onLine(line);
    }
  }
  pending += decoder.decode();
  if (pending.trim() !== '') onLine(pending.trim());
}

/** Parses one NDJSON line of the search; `null` for a line that is not a known event (a newer server). */
export function parseSearchEvent(line: string): SearchEvent | null {
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    return null;
  }
  if (typeof value !== 'object' || value === null || !('type' in value)) return null;
  switch (value.type) {
    case 'start':
    case 'hit':
    case 'progress':
    case 'done':
    case 'error':
      // the server's own tagged union (generated type); the tag was checked above
      return value as SearchEvent;
    default:
      return null;
  }
}

/** `POST /admin/api/items/search` (NDJSON): every event is handed to `onEvent`; abort `signal` to cancel the scan. */
export async function searchItems(
  body: ItemSearchBody,
  onEvent: (event: SearchEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const res = await rest.raw('POST', '/admin/api/items/search', body, signal ? { signal } : {});
  if (!res.body) throw new Error('the server sent no stream');
  await readLines(res.body, (line) => {
    const ev = parseSearchEvent(line);
    if (ev) onEvent(ev);
  });
}
