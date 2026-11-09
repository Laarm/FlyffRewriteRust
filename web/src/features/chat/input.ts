// The input pipeline of the chat window, client half (Plan/45 §8.2; `CWndMgr::ParsingChat`, `CWndChat::Process`): what happens to a
// typed line before `ChatReq` leaves.
//
//  1. leading spaces go; an empty line is nothing,
//  2. the line is cut to `gameplay.chat_max_chars` (120) Unicode scalar values (`SetStrNull`),
//  3. `/r ` or `/R ` at the start becomes `/whisper <last whisperer> ` (no whisperer: unchanged),
//  4. a line starting with `/`: the first token is looked up (names and aliases, ASCII case-insensitive) in the generated command
//     table: a row of side `client` with a registered handler runs here and is NOT sent; so does a server row that has a NAMED
//     browser override (`hasLocalNamed`, only `help` today: BFIX1 #006, the list is a window, not 70 centre notices); `/p` with a
//     party of fewer than two members is refused with `TID_GAME_PARTYNOTCHAT`; everything else (side server or both, unknown
//     commands, emote lines) is sent unchanged, the server decides (23 §3.6, §6.10),
//  5. while `fidelity.chat_filter_server_side` is `false` the legacy sender-side word filter runs (`wordFilter.ts`),
//  6. `ChatReq{text, shortcut}`; there is no client spam timer (PL-110) and no `#` markup stripping.

import { cutChars } from './text';
import { findCommand, type GmCommand } from '../../gen/gm';
import { type WordFilter, filterOutgoing } from './wordFilter';

export type LineRoute =
  /** Nothing to do (an empty line). */
  | { readonly kind: 'empty' }
  /** A browser command of the generated table that has a handler here. */
  | {
      readonly kind: 'local';
      readonly command: GmCommand;
      readonly args: readonly string[];
      readonly line: string;
    }
  /** Refused with a system line (`/p` without a party). */
  | { readonly kind: 'refuse'; readonly tid: number }
  /** Send `ChatReq{text}`. */
  | { readonly kind: 'send'; readonly text: string };

export interface RouteDeps {
  /** `gameplay.chat_max_chars`. */
  readonly maxChars: number;
  readonly lastWhisperer: () => string | null;
  /** The number of members of the own party (`0` without one). */
  readonly partyMembers: () => number;
  /** The authority letter of the own character (commands above it are the server's business). */
  readonly authority: () => number;
  /** `true` when a row has a browser handler. */
  readonly hasLocal: (row: number) => boolean;
  /** `true` when the command of that canonical name has a NAMED browser handler (it then runs here whatever its side). */
  readonly hasLocalNamed?: (name: string) => boolean;
  /** `fidelity.chat_filter_server_side === false`: the client filters (the filter), else `null`. */
  readonly clientFilter: () => WordFilter | null;
  /** `TID_GAME_PARTYNOTCHAT`. */
  readonly partyNotChatTid: number;
}

/** Steps 1 and 2: left trim, the 120-scalar cut. */
export function normalizeInput(raw: string, maxChars: number): string {
  const trimmed = raw.replace(/^[ \t\r\n]+/, '');
  return cutChars(trimmed, maxChars);
}

/** Step 3. */
export function rewriteReply(line: string, lastWhisperer: string | null): string {
  if (lastWhisperer === null || lastWhisperer === '') return line;
  if (line.startsWith('/r ') || line.startsWith('/R ')) {
    return `/whisper ${lastWhisperer} ${line.slice(3)}`;
  }
  return line;
}

/** The first token of a command line (without the slash) and the tokens after it. */
export function tokenize(line: string): { token: string; args: string[] } {
  const body = line.slice(1).trim();
  const parts = body === '' ? [] : body.split(/\s+/);
  return { token: parts[0] ?? '', args: parts.slice(1) };
}

/** `/p` and `/partychat`. */
const isPartyChat = (token: string): boolean => ['p', 'partychat'].includes(token.toLowerCase());

export function routeLine(raw: string, d: RouteDeps): LineRoute {
  let line = normalizeInput(raw, d.maxChars);
  if (line === '') return { kind: 'empty' };
  line = rewriteReply(line, d.lastWhisperer());
  if (line.startsWith('/')) {
    const { token, args } = tokenize(line);
    const cmd = token === '' ? undefined : findCommand(token);
    if (cmd && d.authority() >= cmd.minAuth && d.hasLocalNamed?.(cmd.name) === true) {
      return { kind: 'local', command: cmd, args, line };
    }
    if (cmd?.side === 'client' && d.authority() >= cmd.minAuth && d.hasLocal(cmd.row)) {
      return { kind: 'local', command: cmd, args, line };
    }
    if (isPartyChat(token) && d.partyMembers() < 2) {
      return { kind: 'refuse', tid: d.partyNotChatTid };
    }
  }
  const filter = d.clientFilter();
  return { kind: 'send', text: filter ? filterOutgoing(line, filter) : line };
}
