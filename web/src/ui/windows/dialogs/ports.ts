// Ports of the death and item-handling dialog shells (Plan/42 §19.4 C/D, WP-UI-22; wired by 45 WP-CFEAT-10 inventory drop /
// delete, WP-CFEAT-12 revival, WP-CFEAT-13 upgrade windows). Each dialog is opened with ONE request object: the data it shows
// and the callback of its OK. The decisions behind the callbacks (the sends, the texts of the event rules, the vendor
// bookkeeping) are 45's; the shells keep the control behaviour of the original.

import type { ItemView } from '../../tooltip/types';
import type { ViewSource } from '../char/ports';

/** The item a dialog is about: the tooltip view plus what the dialog draws. */
export interface DialogItem extends ItemView {
  readonly objId: number;
  /** The icon file (`item/itm_x.dds`). */
  readonly icon: string | null;
  /** `m_nItemNum`. */
  readonly count: number;
}

// ---- APP_REVIVAL ---------------------------------------------------------------------------------------------------------------

/** What the revival window shows (read every process, `CWndRevival::OnDraw`). */
export interface RevivalView {
  /** "Revive here" (`WIDC_REVIVAL_STAND`): the resurrection scroll is in the inventory and the world is no arena. */
  readonly standEnabled: boolean;
  /** `EnableButton(WIDC_REVIVAL_TO_LODESTAR, ...)`. */
  readonly lodestarEnabled: boolean;
  /** `EnableButton(WIDC_REVIVAL_TO_LODELIGHT, ...)`: V9 and later hide the lodelight button for good; the flag stays for 45. */
  readonly lodelightEnabled: boolean;
  /** `__S_9_ADD`: the shop button (`ObjectExecutor(SHORTCUT_APPLET, APP_WEBBOX)`). */
  readonly shopVisible: boolean;
}

export type RevivalChoice = 'lodelight' | 'lodestar' | 'stand';

export interface RevivalActions {
  /** `SendHdr(PACKETTYPE_REVIVAL_TO_LODELIGHT / _TO_LODESTAR / PACKETTYPE_REVIVAL)`. */
  revive(choice: RevivalChoice): void;
  /** `SendResurrectionCancel`: sent after the revival when `APP_RESURRECTION` is open. */
  cancelResurrection(): void;
  /** The shop button: `g_WndMng.ObjectExecutor(SHORTCUT_APPLET, APP_WEBBOX)`; the window stays open. */
  shop(): void;
}

export interface RevivalRequest {
  readonly view: ViewSource<RevivalView>;
  readonly actions: RevivalActions;
}

// ---- APP_DROP_ITEM / APP_DROP_CONFIRM / APP_QUEITMWARNING -----------------------------------------------------------------------

export interface DropItemRequest {
  /** `m_nItemNum`: the quantity box starts at the whole stack and is clamped to it. */
  readonly count: number;
  /** The title text of the dialog when it is not the layout's (the event-16 `TID_GAME_DROPITEMREMOVE`); `''`/absent: the layout's. */
  readonly text?: string;
  /** `GetExtra() > 0`: the item became reserved (a trade opened): the window closes itself (`__DROP_CONFIRM_BUG`). */
  reserved(): boolean;
  /** OK / Enter with a quantity of 1..count: `SendDropItem(0, objId, num, pos)`. */
  confirm(count: number): void;
}

export interface DropConfirmRequest {
  readonly text?: string;
  reserved(): boolean;
  /** Yes: `SendDropItem(0, objId, 1, pos)`. */
  yes(): void;
}

export interface QueItemWarningRequest {
  /** The warning (`TID_GAME_QUEITMWARNING` for a quest item; a skill reset or post item brings its own). */
  readonly text: string;
  /** Yes / Enter: the caller opens the drop confirm or the quantity box. */
  yes(): void;
  no?(): void;
  /** The window went away by any route (Yes, No, the close button, a program close): the skill-point warning closes the skill tree with it. */
  closed?(): void;
}

// ---- APP_INVEN_REMOVE_ITEM -----------------------------------------------------------------------------------------------------

export interface InvenRemoveRequest {
  readonly item: DialogItem;
  /** OK with a quantity of 1..count: `SendRemoveItem(item, num)` (and the vendor bookkeeping of the original). */
  confirm(count: number): void;
  /** `PutString(TID_ERROR_NOT_ITEM_NUM)`: OK with an invalid quantity. */
  invalid?(): void;
}
