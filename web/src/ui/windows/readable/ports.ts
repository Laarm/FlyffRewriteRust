// Ports of the readable-item windows (Plan/42 §19.4 D, WP-UI-37; `CWndTextBook`, `CWndTextScroll`, `CWndTextLetter`, `src:_Interface/WndText.cpp`): a
// book, a scroll and a letter show the text file of their item (`ItemProp.szTextFileName`) on a picture of the object. Accept starts the quest of the
// item (a confirmation box asks first; Yes sends `DoUseItem`), Close closes the window. The original opens them from `OpenTextBook` /
// `OpenTextScroll` / `OpenTextLetter`, which no shipped menu or item calls: the windows exist for the item rows that name a text file.

/** What a readable window shows and does. */
export interface ReadableRequest {
  /** The item's name (the caption of the window is the layout's: "Book" / "Scroll" / "Letter"). */
  readonly name: string;
  /** The text of the file (read by the feature; an unreadable file is the empty text). */
  readonly text: string;
  /**
   * `pItemProp->dwQuestId != 0 && !pItemElem->m_bItemResist`: the item starts a quest. The Accept button is disabled for any other item.
   */
  readonly acceptable: boolean;
  /** Accept: the feature asks "Do you want to start the quest?" and, on Yes, uses the item (`SendDoUseItem( MAKELONG( 0, objId ), NULL_ID )`). */
  accept(): void;
  closed?(): void;
}
