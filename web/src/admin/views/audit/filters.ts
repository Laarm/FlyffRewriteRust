// The filters of the audit log (Plan/25 section 10.2): the form state, its deep-link parameters and the query string of
// `GET /admin/api/audit` and `/audit/export`. Pure; tested.
import { withQuery } from '../../api/rest';
import { GROUPS, type Group } from './api';

export type RangeId = '1h' | '24h' | '7d' | '30d' | 'custom';

export const RANGES: readonly { readonly id: RangeId; readonly label: string }[] = [
  { id: '1h', label: 'Last hour' },
  { id: '24h', label: 'Last 24 hours' },
  { id: '7d', label: 'Last 7 days' },
  { id: '30d', label: 'Last 30 days' },
  { id: 'custom', label: 'Custom range' },
];

export const RANGE_MS: Readonly<Record<Exclude<RangeId, 'custom'>, number>> = {
  '1h': 3_600_000,
  '24h': 86_400_000,
  '7d': 7 * 86_400_000,
  '30d': 30 * 86_400_000,
};

export interface Filters {
  readonly range: RangeId;
  /** `datetime-local` text of a custom range. */
  readonly from: string;
  readonly to: string;
  readonly groups: readonly Group[];
  readonly actor: string;
  readonly character: string;
  readonly account: string;
  readonly serial: string;
  readonly text: string;
}

export const EMPTY_FILTERS: Filters = {
  range: '24h',
  from: '',
  to: '',
  groups: [],
  actor: '',
  character: '',
  account: '',
  serial: '',
  text: '',
};

function isRange(v: string | undefined): v is RangeId {
  return RANGES.some((r) => r.id === v);
}

function isGroup(v: string): v is Group {
  return (GROUPS as readonly string[]).includes(v);
}

/** The filters a deep link names (`#/audit/log?character=5&groups=admin,item&range=7d&q=text`); other keys are the shell's. */
export function filtersFromParams(params: Readonly<Record<string, string>>): Filters {
  const range = params['range'];
  const groups = (params['groups'] ?? '')
    .split(',')
    .map((g) => g.trim())
    .filter(isGroup);
  return {
    ...EMPTY_FILTERS,
    range: isRange(range) ? range : EMPTY_FILTERS.range,
    groups,
    actor: params['actor'] ?? '',
    character: params['character'] ?? '',
    account: params['account'] ?? '',
    serial: params['serial'] ?? '',
    text: params['q'] ?? '',
  };
}

export type FilterErrors = Partial<
  Record<'from' | 'to' | 'character' | 'account' | 'serial', string>
>;

export type Built =
  | { readonly ok: true; readonly query: Readonly<Record<string, string | number>> }
  | { readonly ok: false; readonly errors: FilterErrors };

function localTime(text: string): number | null {
  if (text.trim() === '') return null;
  const t = new Date(text).getTime();
  return Number.isNaN(t) ? null : t;
}

/** The query of the filters at `nowMs`, or what is wrong with them. */
export function buildQuery(f: Filters, nowMs: number): Built {
  const errors: { -readonly [K in keyof FilterErrors]: string } = {};
  const query: Record<string, string | number> = {};

  if (f.range === 'custom') {
    const from = localTime(f.from);
    const to = localTime(f.to);
    if (from === null) errors.from = 'Enter the start of the range.';
    if (f.to.trim() !== '' && to === null) errors.to = 'Not a date and time.';
    if (from !== null) query['from'] = from;
    if (to !== null) query['to'] = to;
    if (from !== null && to !== null && from >= to) errors.to = 'The end must be after the start.';
  } else {
    query['from'] = nowMs - RANGE_MS[f.range];
  }

  const ids: [keyof FilterErrors, string, string, number][] = [
    ['character', f.character, 'character', 10],
    ['account', f.account, 'account', 10],
    ['serial', f.serial, 'serial', 20],
  ];
  for (const [key, raw, param, maxDigits] of ids) {
    const v = raw.trim();
    if (v === '') continue;
    if (!/^\d+$/.test(v) || v.length > maxDigits) errors[key] = 'Enter a number.';
    else query[param] = v;
  }

  if (f.groups.length > 0 && f.groups.length < GROUPS.length)
    query['groups'] = GROUPS.filter((g) => f.groups.includes(g)).join(',');
  if (f.actor.trim() !== '') query['actor'] = f.actor.trim();
  if (f.text.trim() !== '') query['q'] = f.text.trim();

  return Object.keys(errors).length > 0 ? { ok: false, errors } : { ok: true, query };
}

/** The download link of an export of the same query (the cookie of the page authenticates it). */
export function exportUrl(
  query: Readonly<Record<string, string | number>>,
  fmt: 'jsonl' | 'csv',
): string {
  return withQuery('/admin/api/audit/export', { ...query, fmt });
}
