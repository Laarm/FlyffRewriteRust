// Ports of the appearance windows (Plan/42 §19.4 L, WP-UI-35; wired by the `appearance` feature of 45, WP-CFEAT-15): the face window, the
// sex-change window and the item-transy window. Each is opened with ONE request object: the data it shows and the callbacks of its buttons.
// The decisions behind the callbacks (the sends, the refusals of the dispatch) are the feature's; the shells keep the control behaviour of
// the original (`src:_Interface/WndChangeFace.cpp`).

import type { DialogItem } from '../dialogs/ports';
import type { PreviewPort } from '../login/ports';

/** The faces of the face window and the sex-change window: `MAX_FACE` (the `WndChangeFace.tga` picture holds five, 86 px apart). */
export const CHANGE_FACE_COUNT = 5;

// ---- APP_CHANGEFACE ----------------------------------------------------------------------------------------------------------------

/**
 * `CWndChangeFace` (464 x 272): a picture of five faces; a click selects one (framed in blue), OK sends it and closes. The original sends
 * the chat command `/ChangeFace <n>` (`SendChat`): no menu or item opens the window in the shipped client, and the server answers the command
 * with its GM text-command table (where it is commented out).
 */
export interface ChangeFaceRequest {
  /** OK with the selected face 0..4. */
  confirm(face: number): void;
  closed?(): void;
}

// ---- APP_CHANGESEX -----------------------------------------------------------------------------------------------------------------

/** The 3-D preview of the sex-change window: the OTHER sex of the player, with the head `face` (doc 41 preset `changeSex`). */
export interface ChangeSexPreview {
  readonly port: PreviewPort;
  /** The subject for head `face` (0..4): the player's hair and equipment on the model of the other sex. */
  subject(face: number): unknown;
}

/**
 * `CWndChangeSex` (192 x 272): the scroll of sex change (`II_CHR_SYS_SCR_TRANSY`, rule R20). A preview of the other sex stands in
 * `WIDC_STATIC1` and the arrows (`WIDC_LEFT` / `WIDC_RIGHT`) page through the five faces (wrapping); OK sends `DoUseItem(scroll, target, face)`.
 */
export interface ChangeSexRequest {
  readonly preview: ChangeSexPreview | null;
  /** OK with the chosen face 0..4: `SendDoUseItem( m_dwItemId, m_dwObjId, m_nSelectFace )`; the window closes. */
  confirm(face: number): void;
  closed?(): void;
}

// ---- APP_ITEM_TRANSY ---------------------------------------------------------------------------------------------------------------

/** What the transy window reads about the dropped armour and its opposite-gender twin. */
export interface TransyPair {
  readonly source: DialogItem;
  /** The twin as an item (the dropped armour with the twin's id, icon and durability: `m_pItemElemChange`). */
  readonly twin: DialogItem;
  /** The item id of the twin (`SendItemTransy`'s `dwChangeId`). */
  readonly twinItemId: number;
}

/**
 * `CWndItemTransy` (176 x 160): the scroll of item transy (`II_CHR_SYS_SCR_ITEMTRANSY_A` / `_B`, rule R25). Two item slots: the armour to turn
 * (`WIDC_STATIC1`, dropped from the inventory) and the item it becomes (`WIDC_STATIC4`, drawn by the window); OK sends `ItemTransy`.
 */
export interface ItemTransyRequest {
  /** The scroll (reserved while the window is open: `Init(pItemElem)` did `SetExtra( 1 )`). */
  readonly scroll: DialogItem;
  /** The texts: `TID_GAME_TRANSITEM_PUT` over the empty slot, `TID_GAME_TRANSITEM_PUT1` over the empty result. */
  readonly tips: { readonly putItem: string; readonly putScroll: string };
  /**
   * `OnDropIcon`, anywhere on the window: a worn item (`TID_GAME_EQUIPPUT`) or one the inventory does not have is refused, the feature says why
   * and the window shows the forbid cursor. Returns `true` when refused.
   */
  refuse(objId: number): boolean;
  /**
   * An inventory item dropped on the first slot: the opposite-gender twin, or `null` when there is none or the level band of the scroll does not fit
   * (`TID_GAME_ITEM_TRANSY`, `TID_GAME_ITEM_TRANSY_NOT_LEVEL_0` / `_1`: the feature says why); the window then shows the forbid cursor.
   */
  accept(objId: number): TransyPair | null;
  /** `SetExtra( 1 )` / `SetExtra( 0 )` of the armour. */
  reserve(objId: number, on: boolean): void;
  /** OK with both slots filled: `SendItemTransy( armour, scroll, twinItemId, TRUE )`; the window closes. */
  send(armourObjId: number, scrollObjId: number, twinItemId: number): void;
  closed?(): void;
}
