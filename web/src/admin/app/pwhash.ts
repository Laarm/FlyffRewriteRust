// The legacy password hash of the login (Plan/12 section 3.2, Plan/25 section 1.3): `md5_hex_lower(salt + password)`, computed in
// the browser so that the plaintext never reaches the server. The salt is `auth.md5_salt` of `GET /api/bootstrap` (the game's own
// route; `crypto.subtle` is not used, the console also runs on plain http, Plan/02 section 11.7).
import { legacyPwHash } from '../../auth/md5';

/** The server's default `auth.legacy_md5_salt`, used when the bootstrap carries none. */
export const DEFAULT_SALT = 'kikugalanet';

let salt: Promise<string> | null = null;

async function fetchSalt(fetchFn: typeof fetch): Promise<string> {
  const res = await fetchFn('/api/bootstrap', { cache: 'no-store', credentials: 'same-origin' });
  if (!res.ok) throw new Error(`GET /api/bootstrap: HTTP ${String(res.status)}`);
  const body = (await res.json()) as { auth?: { md5_salt?: unknown } };
  const s = body.auth?.md5_salt;
  return typeof s === 'string' && s !== '' ? s : DEFAULT_SALT;
}

/** The salt of this server (fetched once; a failed fetch is retried by the next call). */
export function loadSalt(fetchFn: typeof fetch = (...a) => fetch(...a)): Promise<string> {
  salt ??= fetchSalt(fetchFn).catch((error: unknown) => {
    salt = null;
    throw error;
  });
  return salt;
}

/** The `pwhash` for a password. */
export async function passwordHash(password: string): Promise<string> {
  return legacyPwHash(await loadSalt(), password);
}

/** The account name as the server reads it: trimmed, ASCII lower case (Plan/12 section 3.1). */
export function normalizeAccountName(raw: string): string {
  return raw.trim().replace(/[A-Z]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 32));
}

/** 3 to 15 UTF-8 bytes: the rule of the game's own login and password dialogs (a longer one could not log in to the game). */
export function isValidPassword(password: string): boolean {
  const n = new TextEncoder().encode(password).length;
  return n >= 3 && n <= 15;
}

/** Tests: forgets or presets the cached salt. */
export function setSaltForTests(value: string | null): void {
  salt = value === null ? null : Promise.resolve(value);
}
