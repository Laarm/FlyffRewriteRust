// What the card games (find word, puzzle) read of the bag: one item by object id and the client-side reservation `extra` (`CItemElem::SetExtra`, the count a window has put in its slots).

/** One bag item as the card games see it. */
export interface CardItem {
  readonly objId: number;
  readonly itemId: number;
  /** `m_nItemNum`. */
  readonly num: number;
  /** `GetExtra()`: the count reserved by windows. */
  readonly extra: number;
}

/** The bag and the item data. */
export interface CardFacts {
  /** `g_pPlayer->GetItemId( objId )` (the bag only); `null` for none. */
  item(objId: number): CardItem | null;
  /** `SetExtra( n )`. */
  setExtra(objId: number, extra: number): void;
  /** The icon file of an item (`item/itm_x.dds`), `null` for an item without a prop. */
  icon(itemId: number): string | null;
}
