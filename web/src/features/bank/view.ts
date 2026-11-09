// The read model of the bank window (`BankView` of `ui/windows/bank/ports.ts`, Plan/45 §13.2; WP-CFEAT-20): the store's compartments turned into 42
// cells each, with the usable flag of `CWndBank::ReSetBank` (the own compartment, or a compartment with a character in it while a remote-bank
// mode is active). The view keeps its identity while nothing it shows changed (the window polls it at 10 Hz).

import type { ClientDataIndex } from '../../data';
import type { BankView } from '../../ui/windows/bank/ports';
import { ItemViewCache } from '../common/itemView';
import type { BankState } from './state';
import { BANK_CAPACITY, BANK_SLOTS } from './state';

export interface BankViewEnv {
  data(): Pick<ClientDataIndex, 'item' | 'version'> | null | undefined;
  /** `IsCommBank()`: a remote-bank mode (`SM_BANK15`, `SM_BANK30`, `SM_BANK001`) is active. */
  commBank(): boolean;
  /** `features.new_font_color`. */
  newFontColor(): boolean;
}

export class BankViewBuilder {
  readonly #items: ItemViewCache;
  #last: BankView | null = null;
  #lastKey: readonly unknown[] = [];

  constructor(private readonly env: BankViewEnv) {
    this.#items = new ItemViewCache(() => env.data());
  }

  /** The usable flag of compartment `i` (`CWndBank::ReSetBank`). */
  usable(state: BankState, i: number, commBank: boolean): boolean {
    if (state.slot === i) return true;
    return (state.compartments[i]?.characterId ?? 0) !== 0 && commBank;
  }

  build(state: BankState): BankView {
    const commBank = this.env.commBank();
    const fontColor = this.env.newFontColor();
    const key = [state.version, this.env.data()?.version ?? -1, commBank, fontColor];
    if (
      this.#last &&
      key.length === this.#lastKey.length &&
      key.every((v, i) => Object.is(v, this.#lastKey[i]))
    ) {
      return this.#last;
    }
    const compartments = Array.from({ length: BANK_SLOTS }, (_, i) => {
      const c = state.compartments[i];
      const items = Array.from({ length: BANK_CAPACITY }, (_x, slot) => {
        const it = c?.container.getAtSlot(slot) ?? null;
        return it ? this.#items.view(it) : null;
      });
      return {
        index: i,
        characterId: c?.characterId ?? 0,
        own: state.slot === i,
        usable: this.usable(state, i, commBank),
        gold: c?.gold ?? 0,
        items,
      };
    });
    const view: BankView = { version: state.version, compartments, newFontColor: fontColor };
    this.#last = view;
    this.#lastKey = key;
    return view;
  }
}
