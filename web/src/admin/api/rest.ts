// The typed REST client of the admin console (Plan/25 section 11.2, 11.3). Every request:
// * is same-origin with `credentials: 'same-origin'` (the `flyff_admin` cookie, `Path=/admin`) and `cache: 'no-store'`;
// * carries `X-Flyff-Admin: 1` (the CSRF header a cookie-authenticated mutation needs, 25 section 1.3);
// * sends mutations as `application/json`; a repeatable mutation (the routes marked with a star in 11.3) also sends an
//   `Idempotency-Key` (a retry of the same operation passes the same key and gets the stored result).
// An error answer is `{error: {code, message, details?}}` (11.3) and becomes an `AdminApiError`. A 401 outside the session
// routes and a 428 are reported to the session store through `setRestHooks` (the shell then shows the login or the forced
// password change). The WebSocket client (`api/ws.ts`) is a separate module.
import type {
  ErrorBody,
  ErrorCode,
  ErrorDetails,
  ErrorEnvelope,
  Mutation,
  Page,
  Reason,
} from '../gen/dto';

/** What an error can be besides the server's `ErrorCode`s. */
export type ClientErrorCode = ErrorCode | 'network' | 'malformed';

/** A failed admin API call. */
export class AdminApiError extends Error {
  /** HTTP status; 0 when the request never got an answer. */
  readonly status: number;
  readonly code: ClientErrorCode;
  readonly details: ErrorDetails | null;

  constructor(
    status: number,
    code: ClientErrorCode,
    message: string,
    details: ErrorDetails | null = null,
  ) {
    super(message);
    this.name = 'AdminApiError';
    this.status = status;
    this.code = code;
    this.details = details;
  }

  /** The work package a `needs_feature` answer names. */
  get missingWp(): string | null {
    return this.code === 'needs_feature' ? (this.details?.wp ?? null) : null;
  }
}

/** `true` for an `AdminApiError` with this code. */
export function isApiError(error: unknown, code?: ClientErrorCode): error is AdminApiError {
  return error instanceof AdminApiError && (code === undefined || error.code === code);
}

export interface RestHooks {
  /** A request outside the session routes answered 401: the session is gone. */
  readonly onUnauthorized?: () => void;
  /** A request answered `428 password_change_required`. */
  readonly onPasswordChange?: () => void;
}

export interface RequestOptions {
  /** Query parameters; `undefined` values are left out. */
  readonly query?: Readonly<Record<string, string | number | boolean | undefined>>;
  /** Sends an `Idempotency-Key` (a star route). Pass `idempotencyKey` to repeat an earlier operation. */
  readonly idempotent?: boolean;
  readonly idempotencyKey?: string;
  readonly signal?: AbortSignal;
}

type FetchFn = typeof fetch;

export interface Rest {
  get<T>(path: string, options?: RequestOptions): Promise<T>;
  post<T>(path: string, body?: unknown, options?: RequestOptions): Promise<T>;
  put<T>(path: string, body?: unknown, options?: RequestOptions): Promise<T>;
  patch<T>(path: string, body?: unknown, options?: RequestOptions): Promise<T>;
  delete<T>(path: string, body?: unknown, options?: RequestOptions): Promise<T>;
  /**
   * The raw response of a request that passed the same headers and the same error mapping for non-2xx answers (a NDJSON stream,
   * a file download). The caller reads `response.body`.
   */
  raw(method: string, path: string, body?: unknown, options?: RequestOptions): Promise<Response>;
}

/** Appends the query string. */
export function withQuery(path: string, query: RequestOptions['query']): string {
  if (!query) return path;
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) {
    if (v !== undefined) q.set(k, String(v));
  }
  const text = q.toString();
  if (text === '') return path;
  return `${path}${path.includes('?') ? '&' : '?'}${text}`;
}

/** 16 random bytes as 32 hex digits (an `Idempotency-Key`; `crypto.randomUUID` needs a secure context, the console also runs on plain http). */
export function newIdempotencyKey(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let out = '';
  for (const b of bytes) out += (b < 16 ? '0' : '') + b.toString(16);
  return out;
}

/** Reads a non-2xx answer into an `AdminApiError`. */
async function errorOf(res: Response): Promise<AdminApiError> {
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    // not JSON: a proxy page or an empty body
  }
  const err = (body as Partial<ErrorEnvelope> | null)?.error as Partial<ErrorBody> | undefined;
  if (err && typeof err.code === 'string' && typeof err.message === 'string') {
    return new AdminApiError(res.status, err.code, err.message, err.details ?? null);
  }
  // the older routes of Plan/10 section 13.2 (kick, notice, shutdown, rates, events, accounts) answer `{error: "<token>", detail: "<text>"}`
  const legacy = body as { error?: unknown; detail?: unknown } | null;
  if (legacy && typeof legacy.error === 'string') {
    const detail =
      typeof legacy.detail === 'string' && legacy.detail !== '' ? legacy.detail : legacy.error;
    return new AdminApiError(res.status, legacy.error as ClientErrorCode, detail);
  }
  return new AdminApiError(res.status, 'malformed', `HTTP ${String(res.status)}`);
}

function isSessionRoute(path: string): boolean {
  return path === '/admin/api/session' || path.startsWith('/admin/api/session/');
}

/** Builds a client over `fetchFn` (tests inject one). */
export function createRest(
  fetchFn: FetchFn = (...a) => fetch(...a),
  hooks: () => RestHooks = () => ({}),
): Rest {
  async function raw(
    method: string,
    path: string,
    body?: unknown,
    options: RequestOptions = {},
  ): Promise<Response> {
    const headers: Record<string, string> = { Accept: 'application/json', 'X-Flyff-Admin': '1' };
    const init: RequestInit = {
      method,
      headers,
      credentials: 'same-origin',
      cache: 'no-store',
    };
    if (method !== 'GET' && method !== 'HEAD') {
      headers['Content-Type'] = 'application/json';
      init.body = JSON.stringify(body ?? {});
      if (options.idempotent === true || options.idempotencyKey !== undefined) {
        headers['Idempotency-Key'] = options.idempotencyKey ?? newIdempotencyKey();
      }
    }
    if (options.signal) init.signal = options.signal;
    let res: Response;
    try {
      res = await fetchFn(withQuery(path, options.query), init);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'AbortError') throw error;
      throw new AdminApiError(0, 'network', 'The server did not answer.');
    }
    if (res.ok) return res;
    const error = await errorOf(res);
    if (res.status === 401 && !isSessionRoute(path)) hooks().onUnauthorized?.();
    if (error.code === 'password_change_required') hooks().onPasswordChange?.();
    throw error;
  }

  async function json<T>(
    method: string,
    path: string,
    body: unknown,
    options?: RequestOptions,
  ): Promise<T> {
    const res = await raw(method, path, body, options);
    if (res.status === 204) return undefined as T;
    const text = await res.text();
    if (text === '') return undefined as T;
    try {
      return JSON.parse(text) as T;
    } catch {
      throw new AdminApiError(res.status, 'malformed', 'The server sent a malformed answer.');
    }
  }

  return {
    get: (path, options) => json('GET', path, undefined, options),
    post: (path, body, options) => json('POST', path, body, options),
    put: (path, body, options) => json('PUT', path, body, options),
    patch: (path, body, options) => json('PATCH', path, body, options),
    delete: (path, body, options) => json('DELETE', path, body, options),
    raw,
  };
}

let hooks: RestHooks = {};

/** Installs the session store's reactions to 401 and 428. */
export function setRestHooks(next: RestHooks): void {
  hooks = next;
}

let current: Rest = createRest(undefined, () => hooks);

/** The client every view uses. */
export const rest: Rest = {
  get: (p, o) => current.get(p, o),
  post: (p, b, o) => current.post(p, b, o),
  put: (p, b, o) => current.put(p, b, o),
  patch: (p, b, o) => current.patch(p, b, o),
  delete: (p, b, o) => current.delete(p, b, o),
  raw: (m, p, b, o) => current.raw(m, p, b, o),
};

/** Tests: replaces the client behind `rest` (pass `null` to restore the real one). */
export function setRestForTests(replacement: Rest | null): void {
  current = replacement ?? createRest(undefined, () => hooks);
}

/** The `Mutation<T>` envelope of 11.3. */
export function mutation<T>(body: T, reason?: Reason, confirm?: string): Mutation<T> {
  return {
    body,
    ...(reason ? { reason } : {}),
    ...(confirm !== undefined ? { confirm } : {}),
  };
}

/** One page of a cursor list (`?cursor=&limit=`). */
export async function fetchPage<T>(
  path: string,
  query: RequestOptions['query'],
  cursor?: string | null,
  signal?: AbortSignal,
): Promise<Page<T>> {
  return await rest.get<Page<T>>(path, {
    query: { ...query, ...(cursor ? { cursor } : {}) },
    ...(signal ? { signal } : {}),
  });
}
