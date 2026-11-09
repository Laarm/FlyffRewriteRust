// The text filter of the map (Plan/25 section 3.6, MAY): it DIMS everything that does not match.
//
//   filter := term (" " term)*            term := field (":" | ">" | "<" | "=") value
//   fields: player, monster, npc (name substring), level, guild, job, rank (normal|captain|boss|giant), state (asleep|awake|fighting|dead)
//
//   monster:aibatt level>20 state:awake
//
// Terms are combined with AND, except that the name fields `player`, `monster` and `npc` form one OR group (an entity is a player OR a
// monster of that name). A value with spaces is written in double quotes. A term the grammar does not know is reported and ignored.
import { Flag, Kind, Rank } from './types';

export type FilterField =
  'player' | 'monster' | 'npc' | 'level' | 'guild' | 'job' | 'rank' | 'state';

export type FilterOp = ':' | '>' | '<' | '=';

export interface FilterTerm {
  readonly field: FilterField;
  readonly op: FilterOp;
  readonly value: string;
}

export interface ParsedFilter {
  readonly terms: readonly FilterTerm[];
  /** One message per term that could not be used. */
  readonly errors: readonly string[];
}

const FIELDS: readonly FilterField[] = [
  'player',
  'monster',
  'npc',
  'level',
  'guild',
  'job',
  'rank',
  'state',
];

const RANKS = ['normal', 'captain', 'midboss', 'boss', 'giant', 'guard'] as const;
const STATES = ['asleep', 'awake', 'fighting', 'dead'] as const;

/** The job names by id (the defines of `JOB_*`), lower case. */
export const JOB_NAMES: readonly string[] = [
  'vagrant',
  'mercenary',
  'acrobat',
  'assist',
  'magician',
  'puppeteer',
  'knight',
  'blade',
  'jester',
  'ranger',
  'ringmaster',
  'billposter',
  'psykeeper',
  'elementor',
  'gatekeeper',
  'doppler',
  'knight master',
  'blade master',
  'jester master',
  'ranger master',
  'ringmaster master',
  'billposter master',
  'psykeeper master',
  'elementor master',
  'knight hero',
  'blade hero',
  'jester hero',
  'ranger hero',
  'ringmaster hero',
  'billposter hero',
  'psykeeper hero',
  'elementor hero',
];

const TERM = /([A-Za-z]+)([:<>=])("[^"]*"|\S*)/g;

/** Parses the text of the filter box. An empty text is the empty filter (nothing is dimmed). */
export function parseFilter(text: string): ParsedFilter {
  const terms: FilterTerm[] = [];
  const errors: string[] = [];
  const covered = text.trim();
  if (covered === '') return { terms, errors };
  let consumed = 0;
  for (const m of covered.matchAll(TERM)) {
    const [whole, rawField = '', rawOp = ':', rawValue = ''] = m;
    consumed += whole.length;
    const field = rawField.toLowerCase();
    const value = rawValue.startsWith('"') ? rawValue.slice(1, -1) : rawValue;
    if (!(FIELDS as readonly string[]).includes(field)) {
      errors.push(`unknown field "${rawField}"`);
      continue;
    }
    if (value === '') {
      errors.push(`"${rawField}${rawOp}" needs a value`);
      continue;
    }
    const op = rawOp as FilterOp;
    if (op !== ':' && op !== '=' && field !== 'level') {
      errors.push(`"${rawField}" only takes ":" or "="`);
      continue;
    }
    if (field === 'level' && !/^\d+$/.test(value)) {
      errors.push('level needs a whole number');
      continue;
    }
    if (field === 'rank' && !(RANKS as readonly string[]).includes(value.toLowerCase())) {
      errors.push(`rank is one of ${RANKS.join(', ')}`);
      continue;
    }
    if (field === 'state' && !(STATES as readonly string[]).includes(value.toLowerCase())) {
      errors.push(`state is one of ${STATES.join(', ')}`);
      continue;
    }
    terms.push({ field: field as FilterField, op, value });
  }
  // a word that is not a term at all ("foo")
  const stray = covered.replace(TERM, ' ').trim();
  if (stray !== '' && consumed >= 0) {
    for (const w of stray.split(/\s+/)) errors.push(`"${w}" is not a term (field:value)`);
  }
  return { terms, errors };
}

/** What a filter can read of an entity. */
export interface FilterSubject {
  readonly kind: number;
  readonly sub: number;
  readonly level: number;
  readonly flags: number;
  /** The display name (players: from the name table, others: from the data). */
  readonly name: string;
  /** Players: the guild id (`0` = none or unknown). */
  readonly guildId: number;
}

function numericTest(op: FilterOp, have: number, want: number): boolean {
  if (op === '>') return have > want;
  if (op === '<') return have < want;
  return have === want;
}

function rankMatches(sub: number, want: string): boolean {
  switch (want.toLowerCase()) {
    case 'normal':
      return sub === Rank.NORMAL;
    case 'captain':
      return sub === Rank.CAPTAIN;
    case 'midboss':
      return sub === Rank.MIDBOSS;
    case 'boss':
      // "boss" includes the mid-bosses
      return sub === Rank.BOSS || sub === Rank.MIDBOSS;
    case 'giant':
      return sub === Rank.GIANT;
    case 'guard':
      return sub === Rank.GUARD;
    default:
      return false;
  }
}

function stateMatches(flags: number, kind: number, want: string): boolean {
  const asleep = (flags & Flag.ASLEEP) !== 0;
  const dead = (flags & Flag.DEAD) !== 0;
  switch (want.toLowerCase()) {
    case 'asleep':
      return asleep;
    case 'awake':
      return !asleep && !dead && kind !== Kind.ITEM && kind !== Kind.CTRL;
    case 'fighting':
      return (flags & Flag.FIGHTING) !== 0;
    case 'dead':
      return dead;
    default:
      return false;
  }
}

function jobMatches(sub: number, want: string): boolean {
  const w = want.toLowerCase();
  if (/^\d+$/.test(w)) return sub === Number(w);
  const name = JOB_NAMES[sub];
  return name !== undefined && (name === w || name.startsWith(w));
}

/** `true` when `s` satisfies the term (`player:`, `monster:` and `npc:` are tested as a group by {@link compileFilter}). */
export function termMatches(t: FilterTerm, s: FilterSubject): boolean {
  switch (t.field) {
    case 'player':
      return s.kind === Kind.PLAYER && s.name.toLowerCase().includes(t.value.toLowerCase());
    case 'monster':
      return s.kind === Kind.MONSTER && s.name.toLowerCase().includes(t.value.toLowerCase());
    case 'npc':
      return s.kind === Kind.NPC && s.name.toLowerCase().includes(t.value.toLowerCase());
    case 'level':
      return (
        (s.kind === Kind.PLAYER || s.kind === Kind.MONSTER || s.kind === Kind.NPC) &&
        numericTest(t.op, s.level, Number(t.value))
      );
    case 'guild':
      return s.kind === Kind.PLAYER && /^\d+$/.test(t.value) && s.guildId === Number(t.value);
    case 'job':
      return s.kind === Kind.PLAYER && jobMatches(s.sub, t.value);
    case 'rank':
      return s.kind === Kind.MONSTER && rankMatches(s.sub, t.value);
    case 'state':
      return stateMatches(s.flags, s.kind, t.value);
  }
}

/** A predicate that is `true` for the entities that MATCH (everything matches the empty filter). */
export function compileFilter(p: ParsedFilter): (s: FilterSubject) => boolean {
  const names = p.terms.filter(
    (t) => t.field === 'player' || t.field === 'monster' || t.field === 'npc',
  );
  const rest = p.terms.filter(
    (t) => !(t.field === 'player' || t.field === 'monster' || t.field === 'npc'),
  );
  if (names.length === 0 && rest.length === 0) return () => true;
  return (s) => {
    if (names.length > 0 && !names.some((t) => termMatches(t, s))) return false;
    return rest.every((t) => termMatches(t, s));
  };
}

/** `true` when the filter has at least one usable term. */
export function isActive(p: ParsedFilter): boolean {
  return p.terms.length > 0;
}
