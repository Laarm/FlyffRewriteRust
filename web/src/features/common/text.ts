// Localised text helpers of the feature windows (Plan/45 §2.4, §8.4; Plan/11 §10.1, PL-117): the legacy printf-style
// templates of the `texts` bundle are filled with the positional string arguments the server (or the window) hands over.

import type { TextApi } from '../../data/clientData';
import { CHATSTY } from '../../ui/chat/styles';

/**
 * The chat style of the client-side system lines: the default `dwChatStyle` of `CWndMgr::PutString`, `CHATSTY_GAME` (0x600), which
 * the default etc mask sends to the centre of the screen (42 §13.2; before the chat router existed this was a placeholder 0).
 */
export const SYSTEM_CHAT: number = CHATSTY.GAME;

/** The port of 42's chat router (`ui.chat.putString`, 42 §13.2; the hud-chat work package provides it). */
export interface ChatPort {
  putString(text: string, objId: number | null, color: number, style: number): void;
}

/**
 * One printf conversion of a legacy template: `%s %d %u %ld %lu %I64d %c %f %.Nf %x` (flags and width are accepted and
 * ignored), and `%%`.
 */
const CONVERSION = /%(?:%|[-+ #0]*\d*(?:\.\d+)?(?:I64|ll|l|h)?[sducfxXi])/g;

/**
 * `FormatString`-style substitution: the i-th conversion of `template` is replaced by `args[i]`; a missing argument
 * becomes `""`, extra arguments are ignored; `%%` is a single `%` and consumes no argument (Plan/11 §10.1, PL-117).
 */
export function formatTemplate(
  template: string,
  args: readonly (string | number | bigint)[],
): string {
  let i = 0;
  return template.replace(CONVERSION, (m) => {
    if (m === '%%') return '%';
    const a = args[i];
    i += 1;
    return a === undefined ? '' : String(a);
  });
}

/** `formatTid(tid, args)`: the template of `tid` from the texts bundle with the arguments applied. */
export function formatTid(
  texts: TextApi,
  tid: number,
  args: readonly (string | number | bigint)[] = [],
): string {
  return formatTemplate(texts.text(tid), args);
}

/**
 * `sysChat(tid, args, style)`: the legacy "chat X" - one client-side system line in the colour of the text through
 * `ui.chat.putString(text, null, colour, style)`. Without a chat window (title screens) nothing is printed and the
 * function returns `false`.
 */
export function sysChat(
  texts: TextApi,
  chat: ChatPort | null,
  tid: number,
  args: readonly (string | number | bigint)[] = [],
  style: number = SYSTEM_CHAT,
): boolean {
  if (!chat) return false;
  chat.putString(formatTid(texts, tid, args), null, texts.textColor(tid), style);
  return true;
}
