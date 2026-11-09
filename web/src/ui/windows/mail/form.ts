// The checks of the send tab (`CWndPostSend::OnChildNotify(WIDC_SEND)`, `src:_Interface/WndField.cpp:14907-15003`): pure functions over the typed
// texts, so that the window and its tests agree on the order of the refusals. `strlen` of the original counts bytes; so does `byteLength` (UTF-8), the
// unit of the limits of the server (`QueryPostMail`: title <= 31, text <= 255).

import {
  TID_DIAG_0057,
  TID_GAME_LIMITPENYA,
  TID_GAME_MAILBOX_TEXT_MAX,
  TID_GAME_MAILBOX_TITLE_MAX,
} from '../../../gen/defs/text';
import { MAX_MAIL_GOLD, MAX_MAILTEXT, MAX_MAILTITLE, type MailSendForm } from './ports';

/** `strlen` of the text as the wire carries it. */
export function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

export type SendCheck =
  /** Everything is fine: the confirmation box follows. */
  | { readonly ok: true }
  /** The original returned without a word (no receiver or title, penya that are not digits). */
  | { readonly ok: false; readonly kind: 'silent' }
  /** An upper message box with the text `tid` (`argument` fills its `%d` when there is one). */
  | { readonly ok: false; readonly kind: 'box'; readonly tid: number; readonly argument?: number };

export function checkSendForm(form: MailSendForm): SendCheck {
  // "title and receiver must be there"
  if (byteLength(form.receiver) <= 0 || byteLength(form.title) <= 0) {
    return { ok: false, kind: 'silent' };
  }
  // the penya field takes digits only; the original did not say anything
  if (form.gold.length > 0 && !/^[0-9]+$/.test(form.gold)) return { ok: false, kind: 'silent' };
  if (form.gold.length > 0 && Number(form.gold) > MAX_MAIL_GOLD) {
    return { ok: false, kind: 'box', tid: TID_GAME_LIMITPENYA };
  }
  const name = byteLength(form.receiver);
  if (name < 3 || name > 16) return { ok: false, kind: 'box', tid: TID_DIAG_0057 };
  if (byteLength(form.text) > MAX_MAILTEXT - 1) {
    return { ok: false, kind: 'box', tid: TID_GAME_MAILBOX_TEXT_MAX, argument: MAX_MAILTEXT };
  }
  if (byteLength(form.title) > MAX_MAILTITLE - 1) {
    return { ok: false, kind: 'box', tid: TID_GAME_MAILBOX_TITLE_MAX, argument: MAX_MAILTITLE };
  }
  return { ok: true };
}

/** The penya field as the wire number (`atoi`): 0 for an empty field. */
export function goldOf(form: Pick<MailSendForm, 'gold'>): number {
  return form.gold.length === 0 ? 0 : Number(form.gold);
}

/** `GetStrCut(text, n)` + "...": a name or title cut for the list and the reader (`n` BYTES, never in the middle of a character). */
export function cutText(text: string, bytes: number, ellipsisAt: 'over' | 'atLeast'): string {
  const len = byteLength(text);
  const cut = ellipsisAt === 'over' ? len > bytes : len >= bytes;
  if (!cut) return text;
  let out = '';
  let n = 0;
  for (const ch of text) {
    const w = byteLength(ch);
    if (n + w > bytes) break;
    out += ch;
    n += w;
  }
  return `${out}...`;
}
