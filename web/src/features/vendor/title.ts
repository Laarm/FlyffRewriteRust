// The client check of a shop title (`CWndVendor::OnChildNotify( WIDC_OK )`, `src:_Interface/WndVendor.cpp:312-350`; Plan/45 §15.1, 23 §7 `NameKind::VendorTitle`):
// trimmed, not empty, no control characters, no forbidden word (`InvalidName_ENG.inc`) and only allowed letters (`Letter1_ENG.inc`) on the TYPED text, then the
// chat word filter, then 6 to 32 characters of the FILTERED text. The same order as `flyff_rules::names::validate_name( VendorTitle )`; the server checks
// again and is authoritative. Pure: the rules come in as plain data.

import { TID_DIAG_0011, TID_DIAG_0020 } from '../../gen/defs/text';
import type { WordFilter } from '../chat/wordFilter';

/** The shortest and longest title (characters of the filtered text). */
export const MIN_VENDOR_TITLE = 6;
export const MAX_VENDOR_TITLE = 32;

/** The rule data of the `names` bundle. */
export interface TitleRules {
  /** Lower-cased substrings (`InvalidName_ENG.inc`). */
  readonly invalidSubstrings: readonly string[];
  /** The allowed bytes of a vendor title (`Letter1_ENG.inc`), `null` = no restriction. */
  readonly allowedLetters: string | null;
}

export type TitleCheck =
  { readonly ok: true; readonly title: string } | { readonly ok: false; readonly tid: number };

/** ASCII whitespace trim (`Trim`). */
const trim = (s: string): string => s.replace(/^[\t\n\v\f\r ]+|[\t\n\v\f\r ]+$/g, '');

/** The title to send, or the text id that explains why it is refused. */
export function checkVendorTitle(
  raw: string,
  rules: TitleRules | null,
  filter: WordFilter | null,
): TitleCheck {
  const name = trim(raw);
  if (name === '') return { ok: false, tid: TID_DIAG_0011 };
  // eslint-disable-next-line no-control-regex -- control characters are the rule
  if (/[\u0000-\u001f\u007f]/.test(name)) return { ok: false, tid: TID_DIAG_0020 };
  if (rules) {
    const lower = name.toLowerCase();
    if (rules.invalidSubstrings.some((w) => w !== '' && lower.includes(w))) {
      return { ok: false, tid: TID_DIAG_0020 };
    }
    if (rules.allowedLetters !== null) {
      for (const b of new TextEncoder().encode(name)) {
        if (!rules.allowedLetters.includes(String.fromCharCode(b))) {
          return { ok: false, tid: TID_DIAG_0020 };
        }
      }
    }
  }
  const filtered = filter ? filter.change(name) : name;
  const n = Array.from(filtered).length;
  if (n < MIN_VENDOR_TITLE || n > MAX_VENDOR_TITLE) return { ok: false, tid: TID_DIAG_0011 };
  return { ok: true, title: filtered };
}
