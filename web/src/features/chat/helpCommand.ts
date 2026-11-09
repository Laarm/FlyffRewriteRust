// The browser answer of `/help` (BFIX1 #006; the server's version is `crates/flyff-game/src/gm/cmd/info.rs` and stays for the admin
// console and the admin API). Pure functions over the generated command table `gen/gm.ts`, which carries exactly the commands the
// server's `CommandRegistry::visible_to` lists (status port or client): the list a player sees here is the list the server would send,
// but in a scrollable window (`ui/chat/CommandHelpWnd.ts`) instead of as green centre notices of which only 6 stay.
//
// The order is the server's: minimum authority, then the ASCII-lowercase name compared by code units (never `localeCompare`). The
// generated table keeps the aliases in lower case (`gm/export.rs`), so `/skilllevelAll (slvall)` differs from the server's `(slvAll)`
// in case only.

import { findCommand, type GmCommand, visibleCommands } from '../../gen/gm';

/** The server's reply to an unknown or invisible command (`crates/flyff-game/src/gm/mod.rs` `UNKNOWN_COMMAND`, Plan/23 §3.6). */
export const UNKNOWN_COMMAND = 'Unknown command. Type /help for the list.';

const byCodeUnits = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

/** `visible_to`'s sort key: `(min_auth, name.to_ascii_lowercase())`. */
function compareCommands(a: GmCommand, b: GmCommand): number {
  return a.minAuth - b.minAuth || byCodeUnits(a.name.toLowerCase(), b.name.toLowerCase());
}

/** One row of the list: `/name`, the aliases in parentheses, the usage (`info.rs` lines 15-24). */
function helpLine(c: GmCommand): string {
  let line = `/${c.name}`;
  if (c.aliases.length > 0) line += ` (${c.aliases.join(', ')})`;
  if (c.usage !== '') line += ` ${c.usage}`;
  return line;
}

/** `/help` without an argument: the header and one row per command the authority may use. */
export function helpList(authority: number): { header: string; lines: string[] } {
  const list = visibleCommands(authority).sort(compareCommands);
  return { header: `${String(list.length)} commands available:`, lines: list.map(helpLine) };
}

/**
 * `/help <command>`: the usage line and the help line of the command `token` names (a name or an alias, any case, leading slashes
 * ignored) when the authority may use it, else the unknown line. Only the first argument counts, like the server's `args.token()`.
 */
export function helpOne(token: string, authority: number): string[] {
  const c = findCommand(token.replace(/^\/+/, ''));
  if (!c || authority < c.minAuth) return [UNKNOWN_COMMAND];
  return [`/${c.name} ${c.usage}`.trimEnd(), c.help];
}
