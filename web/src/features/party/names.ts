// The checks of the party name (`CWndPartyChangeName::OnChildNotify`, `src:_Interface/WndPartyChangeTroup.cpp:84-185`, `__RULE_0615`; Plan/45 §22.2;
// WP-CFEAT-34): the same rules as the server's `validate_party_name` (4 to 16 characters, not starting with a digit, ASCII letters and digits
// only, no forbidden substring, only allowed letters). The forbidden-name and letter checks need the `names` bundle: the module passes the wasm
// `LobbyRules` when they are available, the checks below run without them (the server checks again).

import { TID_DIAG_0012, TID_DIAG_0013, TID_DIAG_RULE_0 } from '../../gen/defs/text';
import type { LobbyRules } from '../common/lobbyRules';

/** The shortest and longest party name. */
export const MIN_PARTY_NAME = 4;
export const MAX_PARTY_NAME = 16;

/** The text id that explains why `raw` is refused, `0` for an acceptable name. */
export function partyNameProblem(raw: string): number {
  const name = raw.trim();
  if (name.length < MIN_PARTY_NAME || name.length > MAX_PARTY_NAME) return TID_DIAG_RULE_0;
  if (/^[0-9]/.test(name)) return TID_DIAG_0012;
  if (!/^[A-Za-z0-9]+$/.test(name)) return TID_DIAG_0013;
  return 0;
}

/**
 * The full check: the wasm name rules (which add the forbidden substrings and letters) when `rules` and the bundle bytes are there, else
 * {@link partyNameProblem}. Returns the text id of the refusal, `0` when the name is fine.
 */
export function checkPartyName(
  raw: string,
  rules: LobbyRules | null,
  names: Uint8Array | null,
): number {
  if (rules && names) {
    const r = rules.validateCharacterName(raw, names, MIN_PARTY_NAME, MAX_PARTY_NAME);
    return r.name !== undefined ? 0 : (r.tid ?? partyNameProblem(raw));
  }
  return partyNameProblem(raw);
}
