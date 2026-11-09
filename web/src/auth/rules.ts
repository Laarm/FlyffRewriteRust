// Client-side account and password rules (Plan/12 section 3.1, 3.2, 18.1, WP-AUTH-13). The server enforces the same
// rules (the account rule at login: 121; at registration: 2003); checking first saves a round trip and gives the
// original texts: `TID_DIAG_0005` for an account, `TID_DIAG_0030` for a password.

import { TID_DIAG_0005, TID_DIAG_0030 } from '../gen/defs/text';

/** ASCII whitespace of Rust's `u8::is_ascii_whitespace`. */
const TRIM = /^[ \t\n\f\r]+|[ \t\n\f\r]+$/g;

/** The account name as the server reads it: trimmed, ASCII lower case (12 section 3.1 step 1). */
export function normalizeAccount(raw: string): string {
  return raw.replace(TRIM, '').replace(/[A-Z]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 32));
}

export interface LengthRules {
  readonly min: number;
  readonly max: number;
}

/** `bootstrap.auth` lengths with the legacy defaults (3..16 for accounts, 3..15 bytes for passwords). */
export const DEFAULT_ACCOUNT_RULES: LengthRules = { min: 3, max: 16 };
export const DEFAULT_PASSWORD_RULES: LengthRules = { min: 3, max: 15 };

/**
 * `^[a-z][a-z0-9]{2,15}$` after `normalizeAccount` (12 section 3.1): 3 to 16 characters, the first a letter, the rest
 * letters or digits. The lengths come from the bootstrap when it has them.
 */
export function isValidAccount(raw: string, rules: LengthRules = DEFAULT_ACCOUNT_RULES): boolean {
  const name = normalizeAccount(raw);
  return name.length >= rules.min && name.length <= rules.max && /^[a-z][a-z0-9]*$/.test(name);
}

/** The TID to show for an invalid account name, `null` when it is valid. */
export function accountError(raw: string, rules?: LengthRules): number | null {
  return isValidAccount(raw, rules) ? null : TID_DIAG_0005;
}

/** 3 <= UTF-8 bytes <= 15 (legacy `IsPwdValid` rejects `len < 3 || len >= 16`). */
export function isValidPassword(
  password: string,
  rules: LengthRules = DEFAULT_PASSWORD_RULES,
): boolean {
  const bytes = new TextEncoder().encode(password).length;
  return bytes >= rules.min && bytes <= rules.max;
}

/** The TID to show for an invalid password, `null` when it is valid. */
export function passwordError(password: string, rules?: LengthRules): number | null {
  return isValidPassword(password, rules) ? null : TID_DIAG_0030;
}

/** The registration e-mail rule of the server (12 section 6.5 step 6): optional, contains `@`, no whitespace, <= 100 B. */
export function isValidEmail(email: string): boolean {
  return (
    email === '' ||
    (email.includes('@') && !/\s/.test(email) && new TextEncoder().encode(email).length <= 100)
  );
}
