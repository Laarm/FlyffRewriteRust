// The duel fields of the movers (Plan/45 §25.1, ref/09a §5.8; WP-CFEAT-36): `m_nDuel`, `m_nDuelState`, `m_idDuelOther` and
// `m_idDuelParty` of every mover the client knows. The own record drives the HUD, the attack gate (`rules.clientAttackable` reads
// `duelState` / `duelOther` / `duelParty`) and the enemy markers (`XI_GEN_PVP_FLAG01` above the opponents, 41); the records of the
// others are what `SetDuel` and the duel messages wrote on them. Plain data, no React, no context: the reducers of `reducers.ts` and the
// module work on it.

import { NULL_ID } from '../store';

/** The duel fields of one mover. */
export interface DuelFields {
  /** `m_nDuel`: 0 none, 1 solo, 2 party. */
  readonly duel: number;
  /** `m_nDuelState`: 104..100 countdown, 1 fighting, 300 out (party duel), 0 none. */
  readonly state: number;
  /** `m_idDuelOther` (`NULL_ID` for none). */
  readonly other: number;
  /** `m_idDuelParty` (0 none). */
  readonly party: number;
}

export const NO_DUEL: DuelFields = { duel: 0, state: 0, other: NULL_ID, party: 0 };

/** `m_nDuelState` of a duel that was just accepted (the countdown runs from 104). */
export const DUEL_STATE_COUNTDOWN = 104;
/** `m_nDuelState` of a duel in progress (damage flows). */
export const DUEL_STATE_FIGHT = 1;
/** `m_nDuelState` of a party-duel member who is out. */
export const DUEL_STATE_OUT = 300;

function same(a: DuelFields, b: DuelFields): boolean {
  return a.duel === b.duel && a.state === b.state && a.other === b.other && a.party === b.party;
}

export class DuelBook {
  readonly #fields = new Map<number, DuelFields>();
  readonly #listeners = new Set<() => void>();
  #version = 0;

  /** Changes whenever a record changed. */
  get version(): number {
    return this.#version;
  }

  subscribe(fn: () => void): () => void {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  }

  #notify(): void {
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }

  /** The fields of a mover; a mover without a record is not in a duel. */
  get(id: number): DuelFields {
    return this.#fields.get(id) ?? NO_DUEL;
  }

  has(id: number): boolean {
    return this.#fields.has(id);
  }

  /** The ids with a record (the probe). */
  ids(): number[] {
    return [...this.#fields.keys()];
  }

  /** Writes the given fields over the record of a mover. */
  set(id: number, patch: Partial<DuelFields>): void {
    const cur = this.get(id);
    const next: DuelFields = { ...cur, ...patch };
    if (same(cur, next)) return;
    if (same(next, NO_DUEL)) this.#fields.delete(id);
    else this.#fields.set(id, next);
    this.#notify();
  }

  /** `CMover::ClearDuel()`: `m_nDuel = 0`, `m_idDuelOther = NULL_ID`, `m_nDuelState = 0` (the party id stays). */
  clearSolo(id: number): void {
    this.set(id, { duel: 0, other: NULL_ID, state: 0 });
  }

  /** `CMover::ClearDuelParty()`: `m_nDuel = 0`, `m_idDuelParty = 0`, `m_nDuelState = 0` (the solo opponent stays). */
  clearParty(id: number): void {
    this.set(id, { duel: 0, party: 0, state: 0 });
  }

  delete(id: number): void {
    if (this.#fields.delete(id)) this.#notify();
  }

  clear(): void {
    if (this.#fields.size === 0) return;
    this.#fields.clear();
    this.#notify();
  }
}
