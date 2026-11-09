// Error texts of the lobby (Plan/12 section 16, WP-AUTH-13): the HTTP codes of `/api/login` and `/api/register`
// (16.1), the `LobbyError` codes of the character select (16.2) and the client default text per code (16.5), used
// when the server sends `tid = 0`. Two codes have no TID in the original text table: their English text is built
// in (2002 registration disabled, 2005 registration rate limit).

import {
  TID_15MIN_PREVENT,
  TID_15SEC_PREVENT,
  TID_DB_INSPECTION,
  TID_DIAG_0005,
  TID_DIAG_0015,
  TID_DIAG_0016,
  TID_DIAG_0018,
  TID_DIAG_0020,
  TID_DIAG_0030,
  TID_DIAG_0032,
  TID_DIAG_0034,
  TID_DIAG_0035,
  TID_DIAG_0038,
  TID_DIAG_0039,
  TID_DIAG_0041,
  TID_DIAG_0053,
  TID_DIAG_0065,
  TID_DIAG_0073,
  TID_ERROR_CERT_GENERAL,
  TID_GAME_GUILDWARDOING,
} from '../gen/defs/text';
import type { UiNotice } from '../stores/screen';

/** Built-in strings of the codes that have no TID (Plan/12 section 16.1). */
export const BUILTIN_TEXT: Readonly<Record<number, string>> = {
  2002: 'Registration is disabled on this server.',
  2005: 'Too many registrations from this address. Please try again later.',
};

/** The default TID per code (Plan/12 section 16.5); 2002 and 2005 are built in. */
export const DEFAULT_TID: Readonly<Record<number, number>> = {
  100: TID_DIAG_0032,
  103: TID_DIAG_0065,
  104: TID_DIAG_0039,
  105: TID_DIAG_0018,
  106: TID_DIAG_0039,
  107: TID_DIAG_0035,
  108: TID_DIAG_0041,
  109: TID_DIAG_0053,
  111: TID_DIAG_0020,
  112: TID_GAME_GUILDWARDOING,
  119: TID_DIAG_0015,
  120: TID_DIAG_0016,
  121: TID_DIAG_0038,
  133: TID_DB_INSPECTION,
  134: TID_15SEC_PREVENT,
  135: TID_15MIN_PREVENT,
  136: TID_ERROR_CERT_GENERAL,
  1316: TID_DIAG_0034,
  1319: TID_DIAG_0039,
  2000: TID_DIAG_0039,
  2001: TID_DIAG_0073,
  2003: TID_DIAG_0005,
  2004: TID_DIAG_0030,
};

/** Any other code shows this ("Oops. Try again."). */
export const FALLBACK_TID = TID_DIAG_0039;

/** The codes the lobby documents (12 section 16), for tests and tables. */
export const LOBBY_CODES: readonly number[] = [
  ...Object.keys(DEFAULT_TID).map(Number),
  ...Object.keys(BUILTIN_TEXT).map(Number),
];

/** `true` for a `/api/login` answer 103: "This account is already connected. Cut the existing connection?". */
export const CODE_DUPLICATE_ACCOUNT = 103;

/**
 * The text for a lobby error: `tid` (the server's choice) when non-zero, else the default text of `code` (16.5),
 * else the built-in English text, else "Oops. Try again.". `getText` is `ClientDataIndex.text` (before the `texts`
 * bundle is in it answers `"error"`: the built-in text or a short generic line is shown instead).
 */
export function lobbyErrorText(
  code: number,
  tid: number,
  getText: (tid: number) => string,
): string {
  const resolve = (id: number): string | null => {
    const t = getText(id);
    return t === '' || t === 'error' ? null : t;
  };
  if (tid !== 0) {
    const t = resolve(tid);
    if (t !== null) return t;
  }
  const builtin = BUILTIN_TEXT[code];
  if (builtin !== undefined && tid === 0) return builtin;
  const def = DEFAULT_TID[code] ?? FALLBACK_TID;
  return resolve(def) ?? builtin ?? 'Oops. Try again.';
}

/** The notice of the login screen for a failure (`UiNotice`: a TID, or built-in text for 2002 and 2005). */
export function lobbyNotice(code: number, tid: number): UiNotice {
  if (tid !== 0) return { tid };
  const builtin = BUILTIN_TEXT[code];
  if (builtin !== undefined) return { tid: 0, text: builtin };
  return { tid: DEFAULT_TID[code] ?? FALLBACK_TID };
}
