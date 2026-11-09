// Tab completion of the console (Plan/25 section 9.9): the console words of Plan/10 section 13.1 and doc 23, their second words,
// and the GM command names of the generated command table (`gen/gm.ts`), limited to what the session's authority may run.
import { GM_COMMANDS } from '../../../gen/gm';

/** The runtime and console words (`admin::lines`, in the order of `help`). */
export const CONSOLE_WORDS = [
  'help',
  'status',
  'sessions',
  'save-all',
  'set',
  'unset',
  'reload',
  'log',
  'shutdown',
  'backup',
  'db',
  'digest',
  'version',
  'online',
  'kick',
  'account',
  'grant',
  'rates',
  'events',
  'notice',
] as const;

/** The second words of the words that have them. */
export const SECOND_WORDS: Readonly<Record<string, readonly string[]>> = {
  db: ['release', 'checkpoint', 'maintenance'],
  shutdown: ['now', 'cancel'],
  account: ['create', 'passwd', 'set-auth', 'list', 'ban'],
  rates: ['exp', 'drop', 'gold', 'hit', 'shop', 'party', 'aggressive'],
};

/** The lowest authority byte that runs a word (`admin::lines::word_min_authority`: L, M or P). */
export const WORD_MIN: Readonly<Record<string, number>> = {
  help: 76,
  status: 76,
  sessions: 76,
  version: 76,
  online: 76,
  rates: 76,
  events: 76,
  kick: 77,
  notice: 77,
};
const P = 80;

export function wordMin(word: string): number {
  return WORD_MIN[word] ?? P;
}

/** The names a GM line can start with, with aliases, for authority `byte` (commands the browser runs itself are left out). */
export function gmNames(authority: number): readonly string[] {
  const names: string[] = [];
  for (const c of GM_COMMANDS) {
    if (c.side === 'client' || c.minAuth > authority) continue;
    names.push(c.name, ...c.aliases);
  }
  return names;
}

export interface Completion {
  /** Where the replaced word starts in the input. */
  readonly from: number;
  /** Everything that continues the word, sorted. */
  readonly candidates: readonly string[];
}

/** The longest text every candidate starts with. */
export function commonPrefix(list: readonly string[]): string {
  const first = list[0];
  if (first === undefined) return '';
  let end = first.length;
  for (const s of list) {
    while (end > 0 && !s.toLowerCase().startsWith(first.slice(0, end).toLowerCase())) end -= 1;
  }
  return first.slice(0, end);
}

/** What Tab completes in `input` for a session whose authority byte is `authority`. */
export function complete(input: string, authority: number): Completion {
  const lead = /^\s*/.exec(input)?.[0].length ?? 0;
  const rest = input.slice(lead);
  const space = rest.search(/\s/);
  if (space === -1) {
    // the first word
    if (rest.startsWith('/')) {
      const typed = rest.slice(1).toLowerCase();
      const c = gmNames(authority)
        .filter((n) => n.toLowerCase().startsWith(typed))
        .map((n) => `/${n}`);
      return { from: lead, candidates: [...new Set(c)].sort() };
    }
    const typed = rest.toLowerCase();
    const words = CONSOLE_WORDS.filter((w) => w.startsWith(typed) && authority >= wordMin(w)).map(
      String,
    );
    const gm = gmNames(authority).filter(
      (n) => n.toLowerCase().startsWith(typed) && !words.includes(n),
    );
    return { from: lead, candidates: [...new Set([...words, ...gm])].sort() };
  }
  // the second word of a console word
  const first = rest.slice(0, space).toLowerCase();
  const tail = rest.slice(space);
  const afterSpace = tail.trimStart();
  if (/\s/.test(afterSpace)) return { from: input.length, candidates: [] };
  const options = SECOND_WORDS[first];
  if (!options || authority < wordMin(first)) return { from: input.length, candidates: [] };
  const from = input.length - afterSpace.length;
  return {
    from,
    candidates: options.filter((o) => o.startsWith(afterSpace.toLowerCase())),
  };
}

/** Applies a completion to the input: the whole word when there is one candidate (plus a space), else the common prefix. */
export function applyCompletion(input: string, c: Completion): string {
  const only = c.candidates.length === 1 ? c.candidates[0] : undefined;
  if (only !== undefined) return `${input.slice(0, c.from)}${only} `;
  const prefix = commonPrefix(c.candidates);
  return prefix === '' ? input : `${input.slice(0, c.from)}${prefix}`;
}
