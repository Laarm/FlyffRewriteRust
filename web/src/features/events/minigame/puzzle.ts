// The puzzle machine (`CWndPuzzleGame`, `src:_Interface/WndMiniGame.cpp:1598-1950`, `Neuz/DPClient.cpp:11532-11555`; Plan/45 §28.0; WP-CFEAT-45): nine slots (a 3 x 3 picture) for the cards the
// server names with `REASSEMBLE_OPENWND { card_item_ids }`. Nothing is accepted before that message (FIX: the C++ compared against uninitialised slots). A fed or dropped stack fills EVERY empty slot
// whose card equals its item id, up to its count, and its reservation (`extra`) is all the slots it fills (FIX: the C++ SET the extra to the count of the last call); a double click on a filled slot
// empties every slot of that card and releases the stack (FIX: null-safe). Start is enabled iff all nine slots are filled (FIX: the C++ double click did not re-check) and sends
// `ReassembleStart { item_obj_ids }` in slot order. A failed reassemble gets NO reply: the window stays with Start disabled until the slots change or it is closed (KEEP).

import { TID_GAME_MINIGAME_EMPTY_ERROR } from '../../../gen/defs/text';
import type { PuzzleRequest, PuzzleView } from '../../../ui/windows/minigame/ports';
import type { CardFacts } from './cards';

/** The number of slots. */
export const PUZZLE_SLOTS = 9;

export interface PuzzleOut {
  box(tid: number): void;
  /** `SendReassembleStart`. */
  sendStart(objIds: readonly number[]): void;
}

export class PuzzleMachine implements PuzzleRequest {
  /** `m_itemID`: the card of each slot, `null` until `REASSEMBLE_OPENWND`. */
  #cards: readonly number[] | null = null;
  /** The object id in each slot, `null` = empty. */
  readonly slots: (number | null)[] = Array.from({ length: PUZZLE_SLOTS }, () => null);
  #sent = false;
  #released = false;

  constructor(
    readonly facts: CardFacts,
    readonly out: PuzzleOut,
    readonly description: string,
  ) {}

  /** `SetPuzzle`: the nine card ids (a shorter list leaves the rest unusable, a longer one is cut). */
  setCards(ids: readonly number[]): void {
    this.#cards = ids.slice(0, PUZZLE_SLOTS);
    this.#sent = false;
  }

  get cards(): readonly number[] | null {
    return this.#cards;
  }

  #full(): boolean {
    return (
      this.#cards !== null &&
      this.#cards.length === PUZZLE_SLOTS &&
      this.slots.every((s) => s !== null)
    );
  }

  view(): PuzzleView {
    return {
      slots: this.slots.map((obj, i) => {
        if (obj === null) return null;
        const id = this.#cards?.[i];
        return { icon: id === undefined ? null : this.facts.icon(id) };
      }),
      startEnabled: this.#full() && !this.#sent,
      description: this.description,
    };
  }

  /** `OnDropIcon` / `SetPicture`: the stack fills every empty slot of its card up to its count. */
  drop(objId: number): boolean {
    const cards = this.#cards;
    const it = this.facts.item(objId);
    if (!cards || !it) return false;
    let held = this.slots.filter((s) => s === objId).length;
    let added = 0;
    for (let i = 0; i < PUZZLE_SLOTS; i += 1) {
      if (this.slots[i] !== null || cards[i] !== it.itemId) continue;
      if (held >= it.num) break;
      this.slots[i] = objId;
      held += 1;
      added += 1;
    }
    if (added === 0) return false;
    this.facts.setExtra(objId, held);
    this.#sent = false;
    return true;
  }

  /** `OnLButtonDblClk`: every slot of the card under `slot` is emptied and its stack released. */
  empty(slot: number): void {
    const cards = this.#cards;
    if (!cards || slot < 0 || slot >= PUZZLE_SLOTS) return;
    const obj = this.slots[slot];
    if (obj === null || obj === undefined) return;
    const card = cards[slot];
    const released = new Set<number>();
    for (let i = 0; i < PUZZLE_SLOTS; i += 1) {
      const o = this.slots[i];
      if (cards[i] === card && o !== null && o !== undefined) {
        released.add(o);
        this.slots[i] = null;
      }
    }
    for (const o of released) this.facts.setExtra(o, 0);
    this.#sent = false;
  }

  /** `BTN_START`. */
  start(): void {
    if (!this.#full()) {
      this.out.box(TID_GAME_MINIGAME_EMPTY_ERROR);
      return;
    }
    this.#sent = true;
    this.out.sendStart(this.slots as readonly number[]);
  }

  /** The window ended (the module drops its reference). */
  onClosed: (() => void) | null = null;

  /** `OnDestroy`: every reservation is released. */
  closed(): void {
    if (this.#released) return;
    this.#released = true;
    this.onClosed?.();
    for (let i = 0; i < PUZZLE_SLOTS; i += 1) {
      const obj = this.slots[i];
      if (obj !== null && obj !== undefined) this.facts.setExtra(obj, 0);
      this.slots[i] = null;
    }
  }
}
