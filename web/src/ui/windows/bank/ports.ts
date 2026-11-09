// Ports of the bank windows (Plan/42 §19.4 E, WP-UI-25; wired by the `bank` feature of 45, WP-CFEAT-20): the bank (`APP_COMMON_BANK`), the PIN
// entry (`APP_CONFIRM_BANK`) and the PIN window (`APP_BANK_PASSWORD`, first PIN and change). The READ MODEL (`BankView`, observed through a
// `ViewSource`) and the ACTIONS the shells call; plain TS interfaces without protobuf types. Every action names the legacy send behind it
// (ref/99 App. B); the decisions (the quantity boxes, the gates, the sends, the close of the session) are the feature's.

import type { Shortcut } from '../../dnd/shortcut';
import type { InventoryItem } from '../char/ports';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** `MAX_CHARACTER_LIST` / the account's character slots: three compartments, 42 items each (`g_pPlayer->m_Bank[3]`). */
export const BANK_SLOTS = 3;
export const BANK_CAPACITY = 42;

/** A PIN is four digits (`strlen == 4`, `isdigit2` on each). */
export const PIN_LENGTH = 4;

/** One compartment of the bank: the items by slot, the penya and whether the player may use it. */
export interface BankCompartmentView {
  /** The account character slot 0..2 (what `PutItemBack.slot` carries). */
  readonly index: number;
  /** `m_idPlayerBank[i]`: the character that owns the compartment, `0` = no character in that slot. */
  readonly characterId: number;
  /** `m_nSlot == i`: the compartment of the logged-in character, always usable. */
  readonly own: boolean;
  /**
   * `bUse[i]` (`CWndBank::ReSetBank`): the own compartment, or one of another character while a remote-bank mode (`SM_BANK15/30/001`)
   * is active. An unusable compartment is greyed out and takes no press or drop.
   */
  readonly usable: boolean;
  /** `m_dwGoldBank[i]`. */
  readonly gold: number;
  /** The items by slot (`null` = empty), `BANK_CAPACITY` entries. */
  readonly items: readonly (InventoryItem | null)[];
}

export interface BankView {
  /** Bumps with every change of the bank. */
  readonly version: number;
  readonly compartments: readonly BankCompartmentView[];
  /** `features.new_font_color`: the colour of the stack counts. */
  readonly newFontColor: boolean;
}

export interface BankActions {
  /**
   * A shortcut dropped on compartment `slot` (a grid, the coin or the gold text of it) that came from the INVENTORY (`WIN_ITEMDROP` of
   * `CWndBank::OnChildNotify`, nID 11): an item (`sc.data != 0`: the stack > 1 asks for the amount, a worn item says
   * `TID_GAME_EQUIPPUT`) -> `SendPutItemBank(slot, objId, n)`; the coin (`sc.data == 0`) asks for the amount -> `SendPutGoldBank(slot, n)`.
   * Returns whether the drop was accepted (otherwise the forbid icon shows).
   */
  putFromInventory(sc: Shortcut, slot: number): boolean;
  /**
   * A shortcut dragged from compartment `sc.userId` and dropped on compartment `slot` of the same window: item ->
   * `SendPutItemBankToBank(from, slot, objId, n)`; the coin -> `SendPutGoldBankToBank(from, slot, n)` (the server needs a remote-bank mode).
   * Returns whether it was accepted.
   */
  moveBetween(sc: Shortcut, slot: number): boolean;
  /**
   * The window was destroyed (close button, Esc, death, teleport, `CloseBankWnd` of the original destructor): the feature ends the bank
   * session (`SendCloseBankWnd`) and closes its quantity box.
   */
  closed(): void;
}

export interface BankRequest {
  readonly view: ViewSource<BankView>;
  readonly actions: BankActions;
}

/** How a PIN window was closed: by the player (Cancel, the close button, Esc, a transactional close) or replaced by the next window of the flow. */
export type PinClose = 'user' | 'replaced';

/**
 * `CWndConfirmBank` (`APP_CONFIRM_BANK`, 272 x 160): one masked field for the four digits. OK or Enter sends the PIN, "Change PIN" opens the
 * PIN window and closes this one, Cancel closes it. The ids are those of the instant-bank item that opened the session (`NULL_ID` and 0 for the banker).
 */
export interface ConfirmBankRequest {
  /** `SendConfirmBank(pin, dwId, dwItemId)` with four digits. */
  confirm(pin: string): void;
  /** `WIDC_CHANGE_PASS`: the feature opens `APP_BANK_PASSWORD` (change mode, the same ids). The window closes itself as `replaced`. */
  changePassword(): void;
  closed?(how: PinClose): void;
}

/**
 * `CWndBankPassword` (`APP_BANK_PASSWORD`, 272 x 208): old PIN, new PIN, confirmation. `initial` = the first PIN (`m_nFlags == 0`): the old
 * field holds "0000" and the focus starts in the new field.
 */
export interface BankPasswordRequest {
  readonly initial: boolean;
  /** `SendChangeBankPass(old, new, dwId, dwItemId)` after the checks of the window (four digits each, the confirmation equals the new PIN). */
  change(oldPin: string, newPin: string): void;
  closed?(how: PinClose): void;
}
