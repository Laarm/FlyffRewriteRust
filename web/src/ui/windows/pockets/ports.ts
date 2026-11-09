// Ports of the pocket window (Plan/42 §19.4 E, WP-UI-25; wired by the `pockets` feature of 45, WP-CFEAT-29): the backpack `APP_BAG_EX` with the basic pocket and
// the two extra bags. The READ MODEL (`PocketView`, observed through a `ViewSource`) and the ACTIONS the shell calls; plain TS interfaces without protobuf
// types. Every action names the legacy send behind it (ref/99 App. B); the decisions (the gates, the quantity boxes, the sends) are the feature's.

import type { Shortcut } from '../../dnd/shortcut';
import type { InventoryItem } from '../char/ports';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** `MAX_POCKET`: the basic pocket and two extra bags. */
export const POCKET_COUNT = 3;

/** One pocket as the window shows it. */
export interface PocketPanelView {
  /** The pocket index 0..2 (what `MoveItemPocket` carries). */
  readonly index: number;
  /** `IsAvailable(index)`: pocket 0 always, the bags while they exist and have not run out (always with `features.perm_extrabags`). */
  readonly available: boolean;
  /** The items by slot (`null` = empty). */
  readonly items: readonly (InventoryItem | null)[];
  /**
   * The text of the bag icon's tooltip (bags 1 and 2): `TID_TOOLTIP_BAG_TIME` with the time that is left while the bag is available, else
   * `TID_TOOLTIP_BAG_INFO` ("Place the Extra Bag icon here."); `null` without a tooltip (pocket 0, and every bag with `features.perm_extrabags`).
   */
  iconTip(): string | null;
}

export interface PocketView {
  /** Bumps with every change of the pockets. */
  readonly version: number;
  readonly pockets: readonly PocketPanelView[];
  /** `features.perm_extrabags`: the bags never run out, there is nothing to put on their icons. */
  readonly permanent: boolean;
  /** `features.new_font_color`. */
  readonly newFontColor: boolean;
}

export interface PocketActions {
  /**
   * A shortcut dropped on the grid of pocket `pocket`, coming from the INVENTORY (`CWndBagEx::OnChildNotify(WIN_ITEMDROP)`, nID 11, frame `APP_INVENTORY`): a
   * worn item says `TID_GAME_EQUIPPUT`, a quest item `TID_GAME_ERROR_DONT_MOVE_QUEST_ITEM_TO_BAG_EX`, a stack asks for the amount, else
   * `SendMoveItem_Pocket(-1, objId, n, pocket)`. Returns whether it was accepted.
   */
  moveIn(sc: Shortcut, pocket: number): boolean;
  /**
   * A shortcut dropped on the grid of pocket `pocket`, coming from ANOTHER pocket (`sc.userId`; frame `APP_BAG_EX`): the same amount box,
   * `SendMoveItem_Pocket(from, objId, n, pocket)`. Returns whether it was accepted.
   */
  moveBetween(sc: Shortcut, pocket: number): boolean;
  /**
   * An item dropped on the icon of bag `pocket` (1 or 2; `CWndBagEx::OnDropIcon`, from the inventory): a pocket item (`IK3_POCKET`) makes the bag
   * available -> `SendAvailPocket(pocket, objId)`; a bag that is already in use says `TID_GAME_ALREADY_USE`. Returns whether it was accepted.
   */
  availBag(sc: Shortcut, pocket: number): boolean;
  /** The window went (`~CWndBagEx`): the feature closes its amount box. */
  closed(): void;
}

export interface PocketPorts {
  readonly view: ViewSource<PocketView>;
  readonly actions: PocketActions;
}
