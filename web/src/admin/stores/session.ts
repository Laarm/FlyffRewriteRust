// The session of the console (Plan/25 section 1.3): who is signed in, with which capabilities, and the flows around it:
// the staff login, the owner-token exchange, sign out and the password change (the `428 password_change_required` rule).
// The REST client reports 401 and 428 here (`setRestHooks`), so every view reacts to a lost session the same way.
import { useMemo } from 'react';
import { create } from 'zustand';
import { isCap, type Cap } from '../gen/caps';
import type { SessionInfo, TotpSetup } from '../gen/dto';
import { isApiError, rest, setRestHooks } from '../api/rest';
import { normalizeAccountName, passwordHash } from '../app/pwhash';
import { normalizeCode } from '../app/totp';
import { S as TEXT } from '../app/strings';
import { toast } from './toasts';

export type SessionPhase = 'loading' | 'anonymous' | 'ready';

interface SessionState {
  readonly phase: SessionPhase;
  readonly info: SessionInfo | null;
  /** A line above the login form ("Your session ended"). */
  readonly notice: string | null;
  /** The reason the last login attempt failed. */
  readonly loginError: string | null;
  /** A login or password change is running. */
  readonly busy: boolean;
  /** Asks the server who we are (first load, after login, after a password change). */
  readonly init: () => Promise<void>;
  /** `code`: the authenticator code of an account with two-factor sign-in; it is sent only when it is not empty. */
  readonly login: (account: string, password: string, code?: string) => Promise<boolean>;
  readonly exchangeToken: (token: string) => Promise<boolean>;
  readonly logout: () => Promise<void>;
  /** `PUT /admin/api/session/password`; resolves with an error text, or `null` on success. */
  readonly changePassword: (oldPassword: string, newPassword: string) => Promise<string | null>;
  /** Re-reads `GET /admin/api/session` (capabilities, acting character, expiry). */
  readonly refresh: () => Promise<void>;
  /** `POST /admin/api/session/totp {step: "begin"}`: a fresh setup key (nothing changes yet). Rejects with the server's error; `totpErrorText` words it. */
  readonly totpBegin: () => Promise<TotpSetup>;
  /** `{step: "confirm"}`: turns two-factor on with a code from the new key and the current password; resolves with an error text, or `null` on success. */
  readonly totpConfirm: (code: string, password: string) => Promise<string | null>;
  /** `DELETE /admin/api/session/totp`: turns it off (a code and the current password); resolves with an error text, or `null`. */
  readonly totpOff: (code: string, password: string) => Promise<string | null>;
}

const SESSION = '/admin/api/session';

/** The text of a refused login (the same for a wrong password, a banned account and a rank below L: the server does not tell). */
function loginErrorText(error: unknown): string {
  if (isApiError(error, 'invalid_credentials')) return 'Invalid account, password or code.';
  if (isApiError(error, 'rate_limited')) {
    const s = error.details?.retry_after_s;
    return s
      ? `Too many attempts. Try again in ${String(s)} seconds.`
      : 'Too many attempts. Try again in a moment.';
  }
  if (isApiError(error, 'network')) return 'The server did not answer.';
  if (isApiError(error, 'forbidden')) return 'Admin access is not allowed from this address.';
  return error instanceof Error ? error.message : String(error);
}

/** The text of a refused two-factor call: the server's own sentence for a rule (`invalid`, `forbidden`), a short one for the rest. */
export function totpErrorText(error: unknown): string {
  if (isApiError(error, 'invalid_credentials')) return TEXT.totp.wrong;
  if (isApiError(error, 'rate_limited')) return TEXT.totp.tooMany;
  if (isApiError(error, 'network')) return 'The server did not answer.';
  return error instanceof Error ? error.message : String(error);
}

export const useSession = create<SessionState>()((set, get) => ({
  phase: 'loading',
  info: null,
  notice: null,
  loginError: null,
  busy: false,

  async init() {
    try {
      const info = await rest.get<SessionInfo>(SESSION);
      set({ phase: 'ready', info, loginError: null });
    } catch (error) {
      if (isApiError(error) && error.status === 401) {
        set({ phase: 'anonymous', info: null });
      } else {
        set({ phase: 'anonymous', info: null, loginError: loginErrorText(error) });
      }
    }
  },

  async login(account, password, code) {
    set({ busy: true, loginError: null });
    try {
      const pwhash = await passwordHash(password);
      const totp = normalizeCode(code ?? '');
      const info = await rest.post<SessionInfo>(SESSION, {
        account: normalizeAccountName(account),
        pwhash,
        ...(totp === '' ? {} : { totp }),
      });
      set({ phase: 'ready', info, busy: false, notice: null });
      return true;
    } catch (error) {
      set({ busy: false, loginError: loginErrorText(error) });
      return false;
    }
  },

  async exchangeToken(token) {
    set({ busy: true, loginError: null });
    try {
      const info = await rest.post<SessionInfo>(SESSION, { token: token.trim() });
      set({ phase: 'ready', info, busy: false, notice: null });
      return true;
    } catch (error) {
      set({ busy: false, loginError: loginErrorText(error) });
      return false;
    }
  },

  async logout() {
    try {
      await rest.delete(SESSION);
    } catch {
      // the cookie may be gone already: the result is the same
    }
    // the implicit owner of the host has no cookie to drop: asking again shows who we are now
    await get().init();
  },

  async changePassword(oldPassword, newPassword) {
    set({ busy: true });
    try {
      const [old_pwhash, new_pwhash] = await Promise.all([
        passwordHash(oldPassword),
        passwordHash(newPassword),
      ]);
      await rest.put(`${SESSION}/password`, { old_pwhash, new_pwhash });
      await get().refresh();
      set({ busy: false });
      return null;
    } catch (error) {
      set({ busy: false });
      if (isApiError(error, 'invalid_credentials')) return 'The current password is wrong.';
      if (isApiError(error, 'invalid')) return 'That password is not allowed.';
      if (isApiError(error, 'rate_limited')) return 'Too many attempts. Try again in a moment.';
      return error instanceof Error ? error.message : String(error);
    }
  },

  async totpBegin() {
    set({ busy: true });
    try {
      return await rest.post<TotpSetup>(`${SESSION}/totp`, { step: 'begin' });
    } finally {
      set({ busy: false });
    }
  },

  async totpConfirm(code, password) {
    set({ busy: true });
    try {
      const pwhash = await passwordHash(password);
      const info = await rest.post<SessionInfo>(`${SESSION}/totp`, {
        step: 'confirm',
        code: normalizeCode(code),
        pwhash,
      });
      set({ info, busy: false });
      return null;
    } catch (error) {
      set({ busy: false });
      return totpErrorText(error);
    }
  },

  async totpOff(code, password) {
    set({ busy: true });
    try {
      const pwhash = await passwordHash(password);
      const info = await rest.delete<SessionInfo>(`${SESSION}/totp`, {
        code: normalizeCode(code),
        pwhash,
      });
      set({ info, busy: false });
      return null;
    } catch (error) {
      set({ busy: false });
      return totpErrorText(error);
    }
  },

  async refresh() {
    try {
      const info = await rest.get<SessionInfo>(SESSION);
      const before = get().info;
      set({ phase: 'ready', info });
      // a cookie that expired on the host itself leaves the owner of the host behind it: the person must see that it is no longer their staff session
      if (before && before.identity !== info.identity) {
        toast.info(`You are ${info.identity} now (${info.authority}): the earlier session ended.`);
      }
    } catch (error) {
      if (isApiError(error) && error.status === 401) {
        set({ phase: 'anonymous', info: null, notice: 'Your session ended. Sign in again.' });
      }
    }
  },
}));

setRestHooks({
  onUnauthorized: () => {
    if (useSession.getState().phase === 'ready') {
      useSession.setState({
        phase: 'anonymous',
        info: null,
        notice: 'Your session ended. Sign in again.',
      });
    }
  },
  onPasswordChange: () => {
    const { info } = useSession.getState();
    if (info && !info.must_change_password)
      useSession.setState({ info: { ...info, must_change_password: true } });
  },
});

/** The capabilities of a session as typed names (names this build does not know are dropped). */
export function capsOf(info: SessionInfo | null): ReadonlySet<Cap> {
  return new Set((info?.caps ?? []).filter(isCap));
}

/** `true` when the session holds `cap`. */
export function hasCap(info: SessionInfo | null, cap: Cap): boolean {
  return info?.caps.includes(cap) ?? false;
}

/** React hook: does the session hold `cap`? Views hide actions with it; the server still checks. */
export function useCan(cap: Cap): boolean {
  return useSession((s) => hasCap(s.info, cap));
}

/** React hook: the capability set (stable while the session info is). */
export function useCaps(): ReadonlySet<string> {
  const info = useSession((s) => s.info);
  return useMemo(() => new Set(info?.caps ?? []), [info]);
}
