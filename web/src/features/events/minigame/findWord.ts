// The find-word machine (`CWndFindWordGame`, `src:_Interface/WndMiniGame.cpp:613-905`, `Neuz/DPClient.cpp:11556-11590`; Plan/45 §28.0; WP-CFEAT-45): five slots for the letter cards `A` .. `Z`
// (`II_SYS_SYS_EVE_A_CARD` 6370 .. `_Z_CARD` 6395). The server names the question (`ALPHABET_OPENWND { question_id, public_pos, public_char, question }`): the question is APPENDED to the
// description, one letter is shown dimmed in its slot (alpha 100) as a hint, and the player fills the slots in order from the bag (a drop on the hovered slot, or a D3 double click of a card = the
// first empty slot); each card put in a slot reserves one of the stack (`extra + 1`), a double click on a filled slot gives it back. Start is enabled iff all five are filled and sends
// `AlphabetStart { question_id, item_obj_ids }` in slot order; the answer (`ALPHABET_RESULT`) is a box and the window closes. FIX: a drop needs `extra < num` too (the C++ could reserve more cards
// than the stack holds). Closing releases every reservation.

import { II_SYS_SYS_EVE_A_CARD, II_SYS_SYS_EVE_Z_CARD } from '../../../gen/defs/item';
import { TID_GAME_MINIGAME_EMPTY_ERROR } from '../../../gen/defs/text';
import type { FindWordRequest, FindWordView } from '../../../ui/windows/minigame/ports';
import type { CardFacts } from './cards';

/** `CMiniGame::ALPHABET_NOTENOUGH_MONEY`, `ALPHABET_FAILED`. */
export const ALPHABET_RESULT = { NOTENOUGH_MONEY: 998, FAILED: 999 } as const;

/** The number of slots. */
export const WORD_SLOTS = 5;

/** Is the item an `A` .. `Z` card? */
export function isLetterCard(itemId: number): boolean {
  return itemId >= II_SYS_SYS_EVE_A_CARD && itemId <= II_SYS_SYS_EVE_Z_CARD;
}

export interface WordOut {
  box(tid: number): void;
  /** `SendAlphabetStart`. */
  sendStart(questionId: number, objIds: readonly number[]): void;
}

export class FindWordMachine implements FindWordRequest {
  /** The object id in each slot, `null` = empty. */
  readonly slots: (number | null)[] = Array.from({ length: WORD_SLOTS }, () => null);
  /** `m_nQuestionID`. */
  questionId = 0;
  #hint: { slot: number; itemId: number } | null = null;
  #description: string;
  /** The Start button was pressed and nothing changed since (it stays disabled). */
  #sent = false;
  #released = false;

  /** @param description the panel `minigamefindworddesc` (the question is appended later) */
  constructor(
    readonly facts: CardFacts,
    readonly out: WordOut,
    description: string,
  ) {
    this.#description = description;
  }

  /** `ALPHABET_OPENWND`: the question, its id and the hint letter. A hint slot outside 0..4 is ignored (FIX: the C++ indexed past the array). */
  setQuestion(questionId: number, publicPos: number, publicChar: number, question: string): void {
    this.questionId = questionId;
    // `m_firstwordID = nChar - 'A' + II_SYS_SYS_EVE_A_CARD`
    const itemId = publicChar - 'A'.charCodeAt(0) + II_SYS_SYS_EVE_A_CARD;
    this.#hint =
      publicPos >= 0 && publicPos < WORD_SLOTS && isLetterCard(itemId)
        ? { slot: publicPos, itemId }
        : null;
    this.#description = this.#description === '' ? question : `${this.#description}\n${question}`;
  }

  /** The cards in the slots, by item id (`null` = empty). */
  #cardAt(slot: number): number | null {
    const obj = this.slots[slot];
    if (obj === null || obj === undefined) return null;
    return this.facts.item(obj)?.itemId ?? null;
  }

  #filled(): boolean {
    return this.slots.every((s) => s !== null);
  }

  view(): FindWordView {
    const hint = this.#hint;
    return {
      slots: this.slots.map((_, i) => {
        const id = this.#cardAt(i);
        return id === null ? null : { icon: this.facts.icon(id) };
      }),
      hint: hint ? { slot: hint.slot, card: { icon: this.facts.icon(hint.itemId) } } : null,
      startEnabled: this.#filled() && !this.#sent,
      description: this.#description,
    };
  }

  /** Puts the bag item `objId` into slot `slot` (it must be an empty slot and a letter card the stack still has a spare of). */
  #put(slot: number, objId: number): boolean {
    const it = this.facts.item(objId);
    if (!it || !isLetterCard(it.itemId)) return false;
    if (slot < 0 || slot >= WORD_SLOTS || this.slots[slot] !== null) return false;
    // FIX: the reservation never exceeds the stack
    if (it.extra >= it.num) return false;
    this.slots[slot] = objId;
    this.facts.setExtra(objId, it.extra + 1);
    this.#sent = false;
    return true;
  }

  /** `OnDropIcon`: onto the hovered slot. */
  drop(slot: number, objId: number): boolean {
    return this.#put(slot, objId);
  }

  /** `SetWord` (the D3 double click of a card): the first empty slot. */
  feed(objId: number): boolean {
    const slot = this.slots.findIndex((s) => s === null);
    return slot >= 0 && this.#put(slot, objId);
  }

  /** `OnLButtonDblClk` on a filled slot: the card goes back (`extra - 1`). */
  empty(slot: number): void {
    const obj = this.slots[slot];
    if (obj === null || obj === undefined) return;
    this.slots[slot] = null;
    const it = this.facts.item(obj);
    if (it) this.facts.setExtra(obj, Math.max(0, it.extra - 1));
    this.#sent = false;
  }

  /** `BTN_START`. */
  start(): void {
    if (!this.#filled()) {
      this.out.box(TID_GAME_MINIGAME_EMPTY_ERROR);
      return;
    }
    this.#sent = true;
    this.out.sendStart(this.questionId, this.slots as readonly number[]);
  }

  /** The window ended (the module drops its reference). */
  onClosed: (() => void) | null = null;

  /** `OnDestroy`: every reservation is released. */
  closed(): void {
    if (this.#released) return;
    this.#released = true;
    this.onClosed?.();
    for (let i = 0; i < WORD_SLOTS; i += 1) {
      const obj = this.slots[i];
      if (obj !== null && obj !== undefined) this.facts.setExtra(obj, 0);
      this.slots[i] = null;
    }
  }
}
