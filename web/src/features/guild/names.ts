// The input checks of the guild windows (`CWndGuildName::OnChildNotify`, `CWndGuildNickName::OnChildNotify`, `src:_Interface/WndGuildName.cpp:105-300`, `CWndGuildPayConfirm`,
// `CWndGuildNotice`, `CWndGuildWarDecl`; Plan/45 §23, Plan/23 §7; WP-CFEAT-37). The pre-checks run in the order of the original on the TRIMMED text: the length (guild 3 to 16
// characters, UD-21: the server's rule, Plan/23 §7 still says 32; nickname 2 to 12) -> `TID_DIAG_0011` (`TID_DIAG_0011_01` for the nickname), a digit first -> `TID_DIAG_0012`, a
// character that is not an ASCII letter or digit -> `TID_DIAG_0013`, then the forbidden names and letters of `InvalidName.inc` -> `TID_DIAG_0020` (needs the wasm lobby rules and the
// `names` bundle; without them the first three checks run alone and the server checks again).

import {
  TID_DIAG_0011,
  TID_DIAG_0011_01,
  TID_DIAG_0012,
  TID_DIAG_0013,
  TID_DIAG_0020,
} from '../../gen/defs/text';
import type { LobbyRules } from '../common/lobbyRules';

/** `NameKind::Guild` (3..=16) and the nickname (2..=12). */
export const MIN_GUILD_NAME = 3;
export const MAX_GUILD_NAME = 16;
export const MIN_NICKNAME = 2;
export const MAX_NICKNAME = 12;
/** `MAX_BYTE_NOTICE`: a notice must be shorter than this many bytes. */
export const MAX_BYTE_NOTICE = 128;
/** `MAX_G_NAME`: a target guild name of a declaration must be shorter than this. */
export const MAX_G_NAME = 48;
/** The largest salary (`0 <= dwSendPenya <= 999999`). */
export const MAX_SALARY = 999_999;

export type NameKind = 'guild' | 'nickname';

/** The text id of the first of the three cheap checks that refuses `raw`, `0` when none does. */
export function nameProblem(raw: string, kind: NameKind): number {
  const name = raw.trim();
  const [min, max] =
    kind === 'guild' ? [MIN_GUILD_NAME, MAX_GUILD_NAME] : [MIN_NICKNAME, MAX_NICKNAME];
  const length = Array.from(name).length;
  if (length < min || length > max) return kind === 'guild' ? TID_DIAG_0011 : TID_DIAG_0011_01;
  if (/^[0-9]/.test(name)) return TID_DIAG_0012;
  if (!/^[A-Za-z0-9]+$/.test(name)) return TID_DIAG_0013;
  return 0;
}

function check(
  raw: string,
  kind: NameKind,
  rules: LobbyRules | null,
  names: Uint8Array | null,
): number {
  const p = nameProblem(raw, kind);
  if (p !== 0) return p;
  if (rules && names) {
    const [min, max] =
      kind === 'guild' ? [MIN_GUILD_NAME, MAX_GUILD_NAME] : [MIN_NICKNAME, MAX_NICKNAME];
    // the wasm rules add the forbidden names and letters; whatever they refuse after the three checks above is `InvalidName`
    const r = rules.validateCharacterName(raw.trim(), names, min, max);
    if (r.name === undefined) return TID_DIAG_0020;
  }
  return 0;
}

/** The text id that explains why `raw` is no guild name, `0` for a good one. */
export function checkGuildName(
  raw: string,
  rules: LobbyRules | null,
  names: Uint8Array | null,
): number {
  return check(raw, 'guild', rules, names);
}

/** The text id that explains why `raw` is no guild nickname, `0` for a good one. */
export function checkNickname(
  raw: string,
  rules: LobbyRules | null,
  names: Uint8Array | null,
): number {
  return check(raw, 'nickname', rules, names);
}

/** `strlen( szNotice ) < MAX_BYTE_NOTICE` over the UTF-8 bytes. */
export function noticeFits(text: string): boolean {
  return new TextEncoder().encode(text).length < MAX_BYTE_NOTICE;
}

/** The size of a text in bytes (the server limits notices and aliases in bytes). */
export function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

export type SalaryCheck = { readonly ok: true; readonly penya: number } | { readonly ok: false };

/**
 * `CWndGuildPayConfirm`: every character a digit (`isdigit2`), then `0 <= atoi <= 999999`; an empty text is the number 0 (the loop never runs, `atoi( "" )` is 0). Anything
 * else is refused: the edit is cleared and `TID_GAME_GUILDONLYNUMBER` opens.
 */
export function checkSalary(text: string): SalaryCheck {
  if (!/^[0-9]*$/.test(text)) return { ok: false };
  if (text.length > 9) return { ok: false };
  const penya = text === '' ? 0 : Number(text);
  return penya <= MAX_SALARY ? { ok: true, penya } : { ok: false };
}

/** `CWndGuildWarDecl`: the target name has 3 or more and fewer than 48 characters, else nothing happens. */
export function warTargetOk(name: string): boolean {
  const n = Array.from(name.trim()).length;
  return n >= 3 && n < MAX_G_NAME;
}
