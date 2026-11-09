// The event flags of the World & Content page (Plan/25 section 7.1, 22 section 3, 23 section 8.5): the names and descriptions of the flags the
// server knows, and which flag can be changed from here. Pure.

/** A row of `GET /admin/api/events`. */
export interface EventRow {
  readonly id: number;
  readonly key: string | null;
  readonly state: number;
  readonly overridden: boolean;
}

export type FlagKind = 'toggle' | 'value' | 'dead';

export interface FlagInfo {
  readonly name: string;
  readonly description: string;
  readonly kind: FlagKind;
}

/** The ids the original reserved (`EVE_*`); the descriptions are those of 22 section 3 and the `EventId` table. */
export const FLAGS: Readonly<Record<number, FlagInfo>> = {
  0: { name: 'EVE_0401A', description: 'A client April-fools text (dead).', kind: 'dead' },
  1: {
    name: '18+ server',
    description:
      'Changes the PvP/PK case computation and the duel and PK menu entries. Forced on in this build and refused for changes at runtime.',
    kind: 'toggle',
  },
  2: {
    name: 'Guild bank',
    description: 'The guild bank, guild-house furniture and tender results.',
    kind: 'toggle',
  },
  3: { name: 'Guild wars', description: 'Guild wars.', kind: 'toggle' },
  4: { name: 'School event', description: 'A school event (dead).', kind: 'dead' },
  5: { name: 'School battle', description: 'A school battle (dead).', kind: 'dead' },
  6: { name: 'No flying monsters', description: 'Never referenced by the game.', kind: 'dead' },
  7: { name: 'No Darkon', description: 'Never referenced by the game.', kind: 'dead' },
  8: { name: 'No guilds', description: 'Never referenced by the game.', kind: 'dead' },
  9: { name: 'Wormon server', description: 'Marks a "Wormon" server.', kind: 'toggle' },
  10: {
    name: 'Monster respawn',
    description:
      'A number: 0 stops every monster respawn on the server, any other value lets them respawn.',
    kind: 'value',
  },
  11: {
    name: 'PK server',
    description: 'Player killing is allowed outside the PvP areas.',
    kind: 'toggle',
  },
  12: { name: 'PK penalties', description: 'The penalties for killing players.', kind: 'toggle' },
  13: { name: 'Kill stealing', description: 'On: kill-steal protection is OFF.', kind: 'toggle' },
  14: { name: 'Double drop event', description: 'An x2 drop event (dead).', kind: 'dead' },
  15: { name: 'Guild siege', description: 'The guild siege (Guild Combat).', kind: 'toggle' },
  16: {
    name: 'Drop destroys items',
    description: 'A player dropping an item destroys it.',
    kind: 'toggle',
  },
  17: {
    name: 'EVENT_1206',
    description: 'A hard-coded 2005/2006 Core event (dead).',
    kind: 'dead',
  },
  18: {
    name: 'EVENT_1219',
    description: 'A hard-coded 2005/2006 Core event (dead).',
    kind: 'dead',
  },
  19: {
    name: 'EVENT_0127',
    description: 'A hard-coded 2005/2006 Core event (dead).',
    kind: 'dead',
  },
  20: {
    name: 'EVENT_0214',
    description: 'A hard-coded 2005/2006 Core event (dead).',
    kind: 'dead',
  },
  21: {
    name: 'Recommendation',
    description: 'The level-up recommendation report.',
    kind: 'toggle',
  },
  22: { name: 'Guild siege 1:1', description: 'The 1:1 guild combat.', kind: 'toggle' },
  23: { name: 'Arena', description: 'The arena NPC.', kind: 'toggle' },
  24: { name: 'Secret room', description: 'The guild secret room.', kind: 'toggle' },
  25: { name: 'Rainbow race', description: 'The rainbow race event.', kind: 'toggle' },
};

/** The flags a runtime change may touch: ids `0..=499` except 1; 500 and above belong to the generic event timer. */
export const MAX_SETTABLE = 499;

/** Event 1 cannot be changed at runtime. */
export const PROTECTED_ID = 1;

export function infoOf(id: number): FlagInfo {
  return (
    FLAGS[id] ?? {
      name: id >= 500 ? `Generic event ${String(id)}` : `Flag ${String(id)}`,
      description:
        id >= 500
          ? 'Set by the generic event timer (propEvent.inc) while its event runs; not changed by hand.'
          : 'An unnamed flag that scripts and NPCs read.',
      kind: 'value',
    }
  );
}

export function isGeneric(id: number): boolean {
  return id > MAX_SETTABLE;
}

/** `true` when this build lets the console change the flag. */
export function settable(id: number): boolean {
  return id !== PROTECTED_ID && !isGeneric(id) && id >= 0;
}

/** The rows in display order: the named flags by id, then the others by id. */
export function sortFlags(rows: readonly EventRow[]): readonly EventRow[] {
  const rank = (r: EventRow): number => (r.key !== null ? 0 : r.id <= MAX_SETTABLE ? 1 : 2);
  return [...rows].sort((a, b) => rank(a) - rank(b) || a.id - b.id);
}

/** `true` for a flag whose state is a switch (0 or 1). */
export function isToggle(row: EventRow): boolean {
  const info = infoOf(row.id);
  return info.kind === 'toggle' || (info.kind === 'dead' && row.state <= 1);
}
