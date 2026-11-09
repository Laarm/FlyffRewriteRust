// The sender-side profanity filter (Plan/45 §8.2 step 5, 23 §6.9; `CWndMgr::WordChange`, WndManager.cpp:3810). It runs only
// while `fidelity.chat_filter_server_side` is `false`; by default (`true`) the server filters (PL-111) and the client sends
// the line untouched.
//
// The word table is `names.chat_filter` of the data bundles (`filter_ENG.inc` as `[word, replacement]` rows). A word made of
// ASCII letters only is matched as a whole token, case-insensitively, and replaced by one `*` per character; every other
// key is replaced as a substring afterwards, in byte order of the keys. Like the original the filter collapses runs of spaces
// and drops leading spaces and the double quotes of quoted tokens (KEEP-QUIRK). The tokeniser is the part of `CScanner` that
// matters here: quoted strings, identifiers, numbers and single punctuation characters.

export type FilterPair = readonly [string, string];

export interface WordFilter {
  /** `WordChange`. */
  change(text: string): string;
  /** The number of alphabetic and of other keys. */
  readonly size: readonly [number, number];
}

const TOKEN = /"([^"]*)"|[A-Za-z_][A-Za-z0-9_]*|[0-9]+(?:\.[0-9]+)?|[^\s]/g;

const isAlphabetic = (s: string): boolean => /^[A-Za-z]+$/.test(s);

const byteCompare = (a: string, b: string): number => {
  const ea = new TextEncoder().encode(a);
  const eb = new TextEncoder().encode(b);
  const n = Math.min(ea.length, eb.length);
  for (let i = 0; i < n; i += 1) {
    const d = (ea[i] ?? 0) - (eb[i] ?? 0);
    if (d !== 0) return d;
  }
  return ea.length - eb.length;
};

export function createWordFilter(pairs: readonly FilterPair[]): WordFilter {
  const alpha = new Set<string>();
  const other = new Map<string, string>();
  for (const [src, dst] of pairs) {
    if (src === '') continue;
    if (isAlphabetic(src)) alpha.add(src.toLowerCase());
    else if (!other.has(src)) other.set(src, dst);
  }
  const ordered = [...other.entries()].sort((a, b) => byteCompare(a[0], b[0]));
  return {
    size: [alpha.size, other.size],
    change(text: string): string {
      let out = '';
      const matches = [...text.matchAll(TOKEN)];
      matches.forEach((m, i) => {
        // a quoted token contributes its inside; its inner words are filtered like top-level ones
        const inner = m[1];
        const parts = inner === undefined ? [m[0]] : innerTokens(inner);
        for (const p of parts)
          out += alpha.has(p.toLowerCase()) ? '*'.repeat(Array.from(p).length) : p;
        const end = m.index + m[0].length;
        const next = matches[i + 1];
        const gap = next ? text.slice(end, next.index) : '';
        if (next && /\s/.test(gap)) out += ' ';
      });
      for (const [k, v] of ordered) {
        if (k !== '') out = out.split(k).join(v);
      }
      return out;
    },
  };
}

/** The tokens of a quoted string (spaces between them survive as single spaces). */
function innerTokens(inner: string): string[] {
  const t = [...inner.matchAll(TOKEN)].map((m) => m[0]);
  return t.length === 0 ? [''] : [t.join(' ')];
}

/**
 * The outgoing line with the filter applied the way the legacy client did it: normal chat as a whole, and the text after the
 * command token of `/s`, `/p` and `/g` lines; never whispers, IMs or other commands.
 */
export function filterOutgoing(text: string, filter: WordFilter): string {
  if (!text.startsWith('/')) return filter.change(text);
  const m = /^(\/\s*)(\S+)(\s+)([\s\S]*)$/.exec(text);
  if (!m) return text;
  const [, slash = '', token = '', gap = '', rest = ''] = m;
  const t = token.toLowerCase();
  if (['s', 'shout', 'p', 'partychat', 'g', 'guildchat'].includes(t)) {
    return `${slash}${token}${gap}${filter.change(rest)}`;
  }
  return text;
}
