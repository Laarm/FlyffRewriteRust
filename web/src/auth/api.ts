// The HTTP side of the lobby (Plan/12 section 6, 18.6, WP-AUTH-13): `GET /api/bootstrap`, `POST /api/login`,
// `POST /api/register`. JSON only; an error answer is `{code, tid, retry_after_ms?, detail?}` (12 section 6.1) and
// becomes a `LobbyFailure`; a network failure becomes a failure with `http: 0` (the login screen shows
// "Oops. Try again."). The password never reaches this module: callers pass the `pwhash` of `legacyPwHash`.
// `fetch` is injectable for tests.

/** A failed lobby request. `http: 0` = the request never got an answer. */
export interface LobbyFailure {
  readonly http: number;
  readonly code: number;
  readonly tid: number;
  readonly retryAfterMs?: number;
  readonly detail?: string;
}

/** `POST /api/login` 200 (Plan/12 section 6.3). */
export interface LoginOk {
  readonly token: string;
  /** The normalised account name. */
  readonly account: string;
  /** `Authority` letter code (`'F'` = 70 ... `'P'` = 80). */
  readonly authority: number;
  readonly tokenGraceS: number;
  readonly serverTimeMs: number;
}

/** `POST /api/register` 201. */
export interface RegisterOk {
  readonly account: string;
}

/** The code of a request that got no answer at all. */
export const NETWORK_FAILURE_CODE = 0;
/** The code of an answer that is not the documented error body (a proxy page, truncated JSON). */
export const MALFORMED_CODE = 2000;

export function isLobbyFailure(r: unknown): r is LobbyFailure {
  return (
    typeof r === 'object' &&
    r !== null &&
    'http' in r &&
    'code' in r &&
    typeof (r as { http: unknown }).http === 'number'
  );
}

type FetchFn = typeof fetch;

function num(v: unknown, fallback = 0): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

/** Reads an error body into a failure; tolerant of anything a proxy might send. */
async function failureOf(res: Response): Promise<LobbyFailure> {
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    // not JSON
  }
  if (typeof body !== 'object' || body === null) {
    return { http: res.status, code: MALFORMED_CODE, tid: 0 };
  }
  const b = body as Record<string, unknown>;
  return {
    http: res.status,
    code: num(b['code'], MALFORMED_CODE),
    tid: num(b['tid']),
    ...(typeof b['retry_after_ms'] === 'number' ? { retryAfterMs: b['retry_after_ms'] } : {}),
    ...(typeof b['detail'] === 'string' ? { detail: b['detail'] } : {}),
  };
}

async function postJson(
  fetchFn: FetchFn,
  url: string,
  body: unknown,
): Promise<Response | LobbyFailure> {
  try {
    return await fetchFn(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
      cache: 'no-store',
      credentials: 'same-origin',
    });
  } catch {
    return { http: 0, code: NETWORK_FAILURE_CODE, tid: 0 };
  }
}

/**
 * `GET /api/bootstrap` (no-store): the parsed JSON, to be read with `readBootstrap` (`app/bootstrap.ts`). Throws on
 * a network error or a non-200 answer (the boot retries after 1, 2 and 4 s, Plan/40 section 3.1 B1a).
 */
export async function fetchBootstrap(fetchFn: FetchFn = fetch): Promise<unknown> {
  const res = await fetchFn('/api/bootstrap', { cache: 'no-store' });
  if (!res.ok) throw new Error(`GET /api/bootstrap: HTTP ${String(res.status)}`);
  return await res.json();
}

/** `POST /api/login {account, pwhash, force}`. */
export async function login(
  account: string,
  pwhash: string,
  force = false,
  fetchFn: FetchFn = fetch,
): Promise<LoginOk | LobbyFailure> {
  const res = await postJson(fetchFn, '/api/login', { account, pwhash, force });
  if (isLobbyFailure(res)) return res;
  if (!res.ok) return await failureOf(res);
  let body: unknown;
  try {
    body = await res.json();
  } catch {
    return { http: res.status, code: MALFORMED_CODE, tid: 0 };
  }
  const b = (typeof body === 'object' && body !== null ? body : {}) as Record<string, unknown>;
  const token = b['token'];
  if (typeof token !== 'string' || token === '') {
    return {
      http: res.status,
      code: MALFORMED_CODE,
      tid: 0,
      detail: 'login answer without a token',
    };
  }
  return {
    token,
    account: typeof b['account'] === 'string' ? b['account'] : account,
    authority: num(b['authority']),
    tokenGraceS: num(b['token_grace_s'], 60),
    serverTimeMs: num(b['server_time_ms']),
  };
}

/** `POST /api/register {account, pwhash, email?}`; the caller logs in afterwards (one code path). */
export async function register(
  account: string,
  pwhash: string,
  email?: string,
  fetchFn: FetchFn = fetch,
): Promise<RegisterOk | LobbyFailure> {
  const body: Record<string, unknown> = { account, pwhash };
  if (email !== undefined && email !== '') body['email'] = email;
  const res = await postJson(fetchFn, '/api/register', body);
  if (isLobbyFailure(res)) return res;
  if (!res.ok) return await failureOf(res);
  try {
    const parsed = (await res.json()) as { account?: unknown };
    return { account: typeof parsed.account === 'string' ? parsed.account : account };
  } catch {
    return { account };
  }
}
