// The session keys of the client (Plan/12 section 18.5, Plan/40 section 11.3, WP-AUTH-13): the login token, the
// account name and the in-world marker live in `sessionStorage` (they survive a reload, not a new tab). All access
// goes through `safeStorage.session` (try/catch, in-memory fallback), so the page works when storage is blocked: the
// session then lasts until the page is reloaded. Never stores the password or its hash (PL-129).

import { safeStorage, type SafeStorage } from './storage';

export const SESSION_KEYS = {
  token: 'flyff.session.token',
  account: 'flyff.session.account',
  inWorld: 'flyff.session.inWorld',
} as const;

/** The character that was in the world when the page went away (the R4 auto-enter and the re-attach hint). */
export interface InWorldMarker {
  readonly idPlayer: number;
  readonly slot: number;
}

export interface SessionKeys {
  token(): string | null;
  setToken(token: string): void;
  account(): string | null;
  setAccount(account: string): void;
  /** `null` when absent or unreadable. */
  inWorld(): InWorldMarker | null;
  /** `null` clears the marker. */
  setInWorld(marker: InWorldMarker | null): void;
  /** Forgets the whole session (token, account, marker): Back to login, a ban, an expired token. */
  clear(): void;
  /** `false` once sessionStorage proved unavailable (the values live in memory only). */
  readonly persistent: boolean;
}

function parseMarker(raw: string | null): InWorldMarker | null {
  if (raw === null) return null;
  try {
    const v = JSON.parse(raw) as Partial<InWorldMarker> | null;
    if (
      v !== null &&
      typeof v === 'object' &&
      typeof v.idPlayer === 'number' &&
      Number.isInteger(v.idPlayer) &&
      typeof v.slot === 'number' &&
      Number.isInteger(v.slot)
    ) {
      return { idPlayer: v.idPlayer, slot: v.slot };
    }
  } catch {
    // unreadable: no marker
  }
  return null;
}

export function createSessionKeys(storage: SafeStorage = safeStorage.session): SessionKeys {
  return {
    token: () => {
      const t = storage.get(SESSION_KEYS.token);
      return t === null || t === '' ? null : t;
    },
    setToken: (token) => {
      storage.set(SESSION_KEYS.token, token);
    },
    account: () => storage.get(SESSION_KEYS.account),
    setAccount: (account) => {
      storage.set(SESSION_KEYS.account, account);
    },
    inWorld: () => parseMarker(storage.get(SESSION_KEYS.inWorld)),
    setInWorld: (marker) => {
      if (marker === null) storage.remove(SESSION_KEYS.inWorld);
      else storage.set(SESSION_KEYS.inWorld, JSON.stringify(marker));
    },
    clear: () => {
      storage.remove(SESSION_KEYS.token);
      storage.remove(SESSION_KEYS.account);
      storage.remove(SESSION_KEYS.inWorld);
    },
    get persistent() {
      return storage.persistent;
    },
  };
}

let shared: SessionKeys | undefined;

/** The page-wide session keys (created lazily, so importing never touches storage). */
export const session: SessionKeys = {
  token: () => (shared ??= createSessionKeys()).token(),
  setToken: (t) => {
    (shared ??= createSessionKeys()).setToken(t);
  },
  account: () => (shared ??= createSessionKeys()).account(),
  setAccount: (a) => {
    (shared ??= createSessionKeys()).setAccount(a);
  },
  inWorld: () => (shared ??= createSessionKeys()).inWorld(),
  setInWorld: (m) => {
    (shared ??= createSessionKeys()).setInWorld(m);
  },
  clear: () => {
    (shared ??= createSessionKeys()).clear();
  },
  get persistent() {
    return (shared ??= createSessionKeys()).persistent;
  },
};
