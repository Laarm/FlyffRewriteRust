// Ports of the exchange window (Plan/42 §19.4 L, WP-UI-36 shell; wired by the `exchange` feature of 45, WP-CFEAT-26): the NPC trade list of the
// item-exchange scripts (`Resource/Exchange_Script.txt`, `APP_DIALOG_EVENT`). The window is opened with ONE request object: the texts of the
// menu (already resolved from the text table) and the callbacks of its buttons. Which items an entry takes and gives is the SERVER's rule
// (Plan/19 §16); the window only names the entry.

/** The result codes of `ExchangeNtf` (`CExchange::EXCHANGE_*`). */
export const EXCHANGE_SUCCESS = 0;
export const EXCHANGE_FAILED = 1;
export const EXCHANGE_INVENTORY_FAILED = 2;
export const EXCHANGE_CONDITION_FAILED = 3;

export interface ExchangeRequest {
  /** The `MMI_*` id of the NPC menu (`SetMMI`). */
  readonly mmi: number;
  /** The list: the text of every set in script order (`GetListTextId`). */
  readonly entries: readonly string[];
  /** The description pages (`GetDescId`), each a text; the arrow button pages through them. */
  readonly pages: readonly string[];
  /**
   * The `RESULTMSG` pair of a set (`GetResultMsg`): `[success, failure]` texts, `null` when the set has fewer than two entries (the server then
   * prints the details itself).
   */
  results(index: number): readonly [string, string] | null;
  /** `TID_GAME_LACKSPACE`: the answer to a full bag. */
  readonly lackSpace: string;
  /** OK with a chosen entry: `SendExchange( mmi, index )`. */
  send(index: number): void;
  /** `OpenMessageBox( text )`. */
  messageBox(text: string): void;
  closed?(): void;
}
