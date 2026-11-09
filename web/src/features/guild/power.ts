// The power matrix of the guild (Plan/45 §23, 21 §5.5; WP-CFEAT-37): five member levels (master, kingpin, captain, supporter, rookie) times seven power bits, kept as the five words
// the server sends and receives (`GuildAuthorityNtf`, `GuildAuthorityReq`). The appellation page edits the five administration bits (member level 0x01, class 0x02, invitation
// 0x04, penya 0x08, item 0x10), the agit page the two guild-house bits (furniture 0x20, upkeep 0x40); both feed ONE draft (FIX: the original kept two copies that each sent all five
// words, and `SetData` never un-checked a box). The master word is sent as received: the master's own boxes are disabled, the draft ignores rank 0.

import { GUILD_RANKS, PF } from '../../ui/windows/guild/ports';

export { GUILD_RANKS, PF };

/** The seven bits of a word in the order of the matrix columns. */
export const POWER_BITS: readonly number[] = [
  PF.MEMBERLEVEL,
  PF.LEVEL,
  PF.INVITATION,
  PF.PENYA,
  PF.ITEM,
  PF.GH_FURNITURE,
  PF.GH_UPKEEP,
];
/** The bits the appellation page edits. */
export const APP_MASK = PF.MEMBERLEVEL | PF.LEVEL | PF.INVITATION | PF.PENYA | PF.ITEM;
/** The bits the agit page edits. */
export const GH_MASK = PF.GH_FURNITURE | PF.GH_UPKEEP;

/** `rank x bit` as booleans: `matrix[rank][column]` for the columns of {@link POWER_BITS}. */
export type PowerMatrix = readonly (readonly boolean[])[];

/** Five words as the matrix of the seven known bits. */
export function matrixOf(words: readonly number[]): PowerMatrix {
  return Array.from({ length: GUILD_RANKS }, (_, rank) =>
    POWER_BITS.map((bit) => ((words[rank] ?? 0) & bit) !== 0),
  );
}

/** The five words of a matrix: the known bits are replaced, every other bit of `base` stays (the master word `0xff` keeps its high bits). */
export function wordsOf(matrix: PowerMatrix, base: readonly number[]): number[] {
  const known = POWER_BITS.reduce((a, b) => a | b, 0);
  return Array.from({ length: GUILD_RANKS }, (_, rank) => {
    let word = (base[rank] ?? 0) & ~known;
    POWER_BITS.forEach((bit, column) => {
      if (matrix[rank]?.[column] === true) word |= bit;
    });
    return word >>> 0;
  });
}

/** Five words (a short or long list is cut or padded with 0). */
export function fiveWords(words: readonly number[]): number[] {
  return Array.from({ length: GUILD_RANKS }, (_, i) => (words[i] ?? 0) >>> 0);
}

/** One word with `bit` set or cleared. */
export function withBit(word: number, bit: number, on: boolean): number {
  return (on ? word | bit : word & ~bit) >>> 0;
}

/**
 * The draft the two power pages edit. `base` is the guild's words (`GuildAuthorityNtf`, the `Guild` record); `words` is the base plus the unsent changes. A new
 * `base` (the server answered) replaces the draft: `rebase`.
 */
export class PowerDraft {
  #base: number[] = [0, 0, 0, 0, 0];
  #words: number[] = [0, 0, 0, 0, 0];

  constructor(base: readonly number[] = []) {
    this.rebase(base);
  }

  /** The guild's words. */
  get base(): readonly number[] {
    return this.#base;
  }
  /** The draft: what the boxes show and what `GuildAuthorityReq` carries. */
  get words(): readonly number[] {
    return this.#words;
  }
  /** The draft differs from the guild's words. */
  get dirty(): boolean {
    return this.#words.some((w, i) => w !== this.#base[i]);
  }

  /** A new record from the server: the draft starts again from it. Returns `true` when the draft changed. */
  rebase(base: readonly number[]): boolean {
    const next = fiveWords(base);
    const changed = next.some((w, i) => w !== this.#words[i]);
    this.#base = next;
    this.#words = next.slice();
    return changed;
  }

  /** A box of the pages. The master word (rank 0) and bits that are not power bits are not editable. Returns `true` when the draft changed. */
  set(rank: number, bit: number, on: boolean): boolean {
    if (!Number.isInteger(rank) || rank < 1 || rank >= GUILD_RANKS) return false;
    if (!POWER_BITS.includes(bit)) return false;
    const cur = this.#words[rank] ?? 0;
    const next = withBit(cur, bit, on);
    if (next === cur) return false;
    this.#words = this.#words.map((w, i) => (i === rank ? next : w));
    return true;
  }

  /** The words to send, `null` when nothing changed (the original sent all five words on every click of OK). */
  toSend(): number[] | null {
    return this.dirty ? this.#words.slice() : null;
  }
}
