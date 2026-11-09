// The card game (`CWndRRMiniGameCard`, `src:_Interface/WndRainbowRace.cpp:1942-2151`, Plan/42 §19.4 P; WP-UI-61) as a pure model: 18 cards in 6 x 3 cells, two picks per try, a mismatch blocks the clicks for five
// seconds (acceptance: blocked at 4 999 ms, allowed at 5 001 ms) and shows the countdown digit `4 - floor( elapsed / 1000 )`.

export const CARD_COLS = 6;
export const CARD_ROWS = 3;
export const CARD_COUNT = 18;
/** `SEC( 5 )`. */
export const PENALTY_MS = 5000;

export class CardModel {
  /** The kind (0..8) of each card, -1 for a card the deck did not name. */
  readonly kinds: number[] = new Array<number>(CARD_COUNT).fill(-1);
  readonly matched: boolean[] = new Array<boolean>(CARD_COUNT).fill(false);
  #first = -1;
  #second = -1;
  #penaltyAt: number | null = null;

  get first(): number {
    return this.#first;
  }
  get second(): number {
    return this.#second;
  }
  get penaltyAt(): number | null {
    return this.#penaltyAt;
  }

  /** `SetQuestion`: card `i` is byte `i` of the deck minus one (a value outside 0..8 draws no picture); every card goes face down. */
  setDeck(deck: string): void {
    for (let i = 0; i < Math.min(deck.length, CARD_COUNT); i += 1) {
      const kind = deck.charCodeAt(i) - 1;
      this.kinds[i] = kind >= 0 && kind < 9 ? kind : -1;
      this.matched[i] = false;
    }
  }

  /** The clicks are blocked while a penalty runs (`now - penaltyAt <= 5000`). */
  blocked(now: number): boolean {
    return this.#penaltyAt !== null && now - this.#penaltyAt <= PENALTY_MS;
  }

  /** A card shows its face when it is matched or picked. */
  faceUp(i: number): boolean {
    return this.matched[i] === true || i === this.#first || i === this.#second;
  }

  /** The cursor over a cell: SELECT for a face-down card while no penalty runs. */
  clickable(i: number, now: number): boolean {
    return !this.matched[i] && !this.blocked(now);
  }

  /**
   * A click (button up) on cell `i`: no first pick -> remember it; else a different cell with no second pick -> the pair to send. FIX: a matched card is ignored (the original let the click through and
   * the server's False cost a penalty).
   */
  click(i: number, now: number): readonly [number, number] | null {
    if (i < 0 || i >= CARD_COUNT || this.blocked(now) || this.matched[i]) return null;
    if (this.#first === -1) {
      this.#first = i;
      return null;
    }
    if (this.#first !== i && this.#second === -1) {
      this.#second = i;
      return [this.#first, this.#second];
    }
    return null;
  }

  /** `ReceiveResult`: TRUE matches both picks, FALSE starts the penalty; every result clears the picks (KEEP: a mismatched pair flips back when the answer arrives). */
  result(state: number, now: number, trueState: number, falseState: number): void {
    if (state === trueState) {
      if (this.#first >= 0) this.matched[this.#first] = true;
      if (this.#second >= 0) this.matched[this.#second] = true;
    } else if (state === falseState) {
      this.#penaltyAt = now;
    }
    this.#first = -1;
    this.#second = -1;
  }

  /** The countdown digit (sprite `4 - floor( elapsed / 1000 )` of the penalty atlas) while the penalty runs, else `null`. */
  penaltyDigit(now: number): number | null {
    if (this.#penaltyAt === null) return null;
    const elapsed = now - this.#penaltyAt;
    if (elapsed < 0 || elapsed >= PENALTY_MS) return null;
    return 4 - Math.floor(elapsed / 1000);
  }
}
