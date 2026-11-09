// The browser auth client (Plan/12 section 18.6, WP-AUTH-13): the pure-TS md5, `/api/*` calls, error texts and the
// client-side account/password rules. The session storage keys are `platform/session.ts`; the login flow that feeds
// the screen state machine is `app/authActions.ts`. Layering rules: eslint/rules.js.
export {
  fetchBootstrap,
  isLobbyFailure,
  login,
  MALFORMED_CODE,
  NETWORK_FAILURE_CODE,
  register,
  type LobbyFailure,
  type LoginOk,
  type RegisterOk,
} from './api';
export {
  BUILTIN_TEXT,
  CODE_DUPLICATE_ACCOUNT,
  DEFAULT_TID,
  FALLBACK_TID,
  LOBBY_CODES,
  lobbyErrorText,
  lobbyNotice,
} from './errors';
export { legacyPwHash, md5, md5Hex } from './md5';
export {
  accountError,
  DEFAULT_ACCOUNT_RULES,
  DEFAULT_PASSWORD_RULES,
  isValidAccount,
  isValidEmail,
  isValidPassword,
  normalizeAccount,
  passwordError,
  type LengthRules,
} from './rules';
