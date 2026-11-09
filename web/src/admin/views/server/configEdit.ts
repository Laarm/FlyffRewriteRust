// The pure parts of the configuration page: how a value is shown, how typed text becomes a value of the key's kind, and the filter.
import type { ConfigClass, ConfigKind, ConfigRow } from './api';

/** A value as the table shows it: strings plain, everything else as compact JSON. */
export function displayValue(v: unknown): string {
  if (v === null || v === undefined) return '(unset)';
  if (typeof v === 'string') return v === '' ? '""' : v;
  return JSON.stringify(v);
}

/** The text a field starts with for a value. */
export function inputText(v: unknown): string {
  if (v === null || v === undefined) return '';
  return typeof v === 'string' ? v : JSON.stringify(v);
}

export type Parsed =
  { readonly ok: true; readonly value: unknown } | { readonly ok: false; readonly error: string };

/** Typed text as the value of a key of `kind` (the server validates the range; this catches what cannot be a value at all). */
export function parseInput(kind: ConfigKind, text: string): Parsed {
  const t = text.trim();
  switch (kind) {
    case 'bool':
      if (t === 'true' || t === 'false') return { ok: true, value: t === 'true' };
      return { ok: false, error: 'Choose true or false.' };
    case 'int': {
      if (!/^-?\d+$/.test(t)) return { ok: false, error: 'A whole number, like 25.' };
      const n = Number(t);
      return Number.isSafeInteger(n)
        ? { ok: true, value: n }
        : { ok: false, error: 'That number is too large.' };
    }
    case 'float': {
      const n = Number(t);
      return t !== '' && Number.isFinite(n)
        ? { ok: true, value: n }
        : { ok: false, error: 'A number, like 1.5.' };
    }
    case 'str':
      return { ok: true, value: text };
    case 'str_array':
    case 'table_array':
    case 'table':
      try {
        const v: unknown = JSON.parse(t);
        return Array.isArray(v) || (typeof v === 'object' && v !== null)
          ? { ok: true, value: v }
          : { ok: false, error: 'A JSON list or object.' };
      } catch {
        return { ok: false, error: 'Not valid JSON.' };
      }
  }
}

export interface ConfigFilter {
  readonly text: string;
  readonly section: string;
  readonly cls: ConfigClass | 'all' | 'overridden';
}

/** Rows that match: the text in the key or the description, the section, the class (or only the runtime overrides). */
export function filterConfig(rows: readonly ConfigRow[], f: ConfigFilter): readonly ConfigRow[] {
  const needle = f.text.trim().toLowerCase();
  return rows.filter((r) => {
    if (f.section !== '' && r.section !== f.section) return false;
    if (f.cls === 'overridden' ? r.source !== 'override' : f.cls !== 'all' && r.class !== f.cls)
      return false;
    return (
      needle === '' || r.key.toLowerCase().includes(needle) || r.doc.toLowerCase().includes(needle)
    );
  });
}

/** The distinct sections in the order of the rows. */
export function sectionsOf(rows: readonly ConfigRow[]): readonly string[] {
  return [...new Set(rows.map((r) => r.section))];
}

/** Replaces the row with the same key (the answer of a save), or leaves the list alone. */
export function replaceRow(
  rows: readonly ConfigRow[],
  row: ConfigRow | null | undefined,
): readonly ConfigRow[] {
  if (!row) return rows;
  return rows.map((r) => (r.key === row.key ? row : r));
}
