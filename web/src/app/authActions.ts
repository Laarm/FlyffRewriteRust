// The login flow of the page (Plan/12 section 18.1, Plan/40 section 4.2, WP-AUTH-13): what the login window (45) calls.
// It checks the client-side rules, computes the legacy hash, posts `/api/login`, stores the token and feeds the screen
// state machine (`loginOk` opens the socket, `loginFailed` shows the notice). A 103 answer ("This account is already
// connected. Would you like to cut the existing connection?") is NOT a failure of the screen: the caller asks the
// player and calls `login(..., { force: true })`, which repeats the request with the password verified again.

import { accountError, passwordError } from '../auth/rules';
import {
  isLobbyFailure,
  login as postLogin,
  register as postRegister,
  type LobbyFailure,
} from '../auth/api';
import { CODE_DUPLICATE_ACCOUNT, lobbyNotice } from '../auth/errors';
import { legacyPwHash } from '../auth/md5';
import { isValidEmail, normalizeAccount } from '../auth/rules';
import { TID_DIAG_0005, TID_DIAG_0030 } from '../gen/defs/text';
import type { UiNotice } from '../stores/screen';
import type { ClientContext } from './context';

export type LoginOutcome =
  /** The token is stored and the socket is opening (`Connecting`). */
  | { readonly kind: 'ok'; readonly account: string }
  /** Code 103: ask whether to cut the existing connection, then `login(..., { force: true })`. */
  | { readonly kind: 'takeover'; readonly notice: UiNotice }
  /** The login screen shows `notice`. */
  | { readonly kind: 'failed'; readonly notice: UiNotice; readonly failure: LobbyFailure | null };

export type RegisterOutcome =
  | { readonly kind: 'ok'; readonly login: LoginOutcome }
  | { readonly kind: 'failed'; readonly notice: UiNotice; readonly failure: LobbyFailure | null };

export interface AuthActions {
  login(account: string, password: string, options?: { force?: boolean }): Promise<LoginOutcome>;
  /** Registers and, on success, logs in with the same credentials (one code path, Plan/12 section 6.5). */
  register(account: string, password: string, email?: string): Promise<RegisterOutcome>;
  /** The legacy hash for the salt of this server (the character delete dialog sends it too). */
  pwHash(password: string): string;
}

type FetchFn = typeof fetch;

export function createAuthActions(ctx: ClientContext, fetchFn: FetchFn = ctx.fetch): AuthActions {
  const salt = (): string => ctx.bootstrap?.auth.md5_salt ?? 'kikugalanet';
  const rules = () => {
    const a = ctx.bootstrap?.auth;
    return {
      account: a ? { min: a.account_min_len, max: a.account_max_len } : undefined,
      password: a ? { min: a.password_min_len, max: a.password_max_len } : undefined,
    };
  };

  const invalid = (tid: number): LoginOutcome => {
    const notice: UiNotice = { tid };
    ctx.machine.dispatch({ type: 'loginFailed', notice });
    return { kind: 'failed', notice, failure: null };
  };

  const login = async (
    rawAccount: string,
    password: string,
    options: { force?: boolean } = {},
  ): Promise<LoginOutcome> => {
    const r = rules();
    if (accountError(rawAccount, r.account) !== null) return invalid(TID_DIAG_0005);
    if (passwordError(password, r.password) !== null) return invalid(TID_DIAG_0030);
    const account = normalizeAccount(rawAccount);
    const answer = await postLogin(
      account,
      legacyPwHash(salt(), password),
      options.force === true,
      fetchFn,
    );
    if (isLobbyFailure(answer)) {
      const notice = lobbyNotice(answer.code, answer.tid);
      if (answer.code === CODE_DUPLICATE_ACCOUNT) return { kind: 'takeover', notice };
      ctx.log.warn(
        'auth',
        `login refused: HTTP ${String(answer.http)} code ${String(answer.code)}`,
      );
      ctx.machine.dispatch({ type: 'loginFailed', notice });
      return { kind: 'failed', notice, failure: answer };
    }
    ctx.session.setToken(answer.token);
    ctx.session.setAccount(answer.account);
    ctx.machine.dispatch({ type: 'loginOk' });
    return { kind: 'ok', account: answer.account };
  };

  return {
    login,
    pwHash: (password) => legacyPwHash(salt(), password),
    async register(rawAccount, password, email) {
      const r = rules();
      const fail = (tid: number): RegisterOutcome => ({
        kind: 'failed',
        notice: { tid },
        failure: null,
      });
      if (accountError(rawAccount, r.account) !== null) return fail(TID_DIAG_0005);
      if (passwordError(password, r.password) !== null) return fail(TID_DIAG_0030);
      if (email !== undefined && !isValidEmail(email)) return fail(2038);
      const account = normalizeAccount(rawAccount);
      const answer = await postRegister(account, legacyPwHash(salt(), password), email, fetchFn);
      if (isLobbyFailure(answer)) {
        return { kind: 'failed', notice: lobbyNotice(answer.code, answer.tid), failure: answer };
      }
      return { kind: 'ok', login: await login(answer.account, password) };
    },
  };
}
