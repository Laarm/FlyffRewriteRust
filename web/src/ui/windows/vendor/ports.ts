// Ports of the personal-shop windows (Plan/42 §19.4 F, WP-UI-26, WP-UI-37; wired by `features/vendor`, WP-CFEAT-22): the READ MODEL of the shop
// window (`VendorView`, observed through a `ViewSource`), its ACTIONS, and one request object per dialog (the price registration, the purchase,
// the chat room). The decisions behind the callbacks (the total-gold rule, the title rules, the sends) are the feature's; the shells keep the control
// behaviour of the originals (`CWndVendor`, `CWndRegVend`, `CWndVendorBuy`, `CWndVendorMessage`, `src:_Interface/WndVendor.cpp`, `WndRegVend.cpp`,
// `WndVendorBuy.cpp`, `WndVendorCtrl.cpp`). Each action names the legacy send it covers (ref/99 App. B).

import type { Shortcut } from '../../dnd/shortcut';
import type { ItemView } from '../../tooltip/types';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** One registered item of a shop (a `CItemElem` of `m_vtInfo` with `m_nCost` and the reserved count as its extra). */
export interface VendorEntry extends ItemView {
  /** The shop slot 0..19. */
  readonly slot: number;
  /** The object id of the item: the owner's inventory entry, or the vendor's entry of the `PVendorItem` list. */
  readonly objId: number;
  readonly itemId: number;
  readonly icon: string | null;
  /** The count for sale (`GetExtra()`). */
  readonly count: number;
  /** The unit price (`m_nCost`). */
  readonly price: number;
  /** `GetAbilityOption()`: shown as `name +n`. */
  readonly option: number;
  /** `dwPackMax > 1`: the count is printed. */
  readonly showCount: boolean;
  readonly expired: boolean;
  /** The record for the tooltip composers (45's `InvItem`). */
  readonly item?: unknown;
}

export interface VendorView {
  /** Bumps with every change. */
  readonly version: number;
  /** `true`: the own shop (set-up and selling); `false`: somebody else's goods are shown (visitor mode: no OK, no Cancel). */
  readonly owner: boolean;
  /** The text of the title edit: the title of the visited shop, or what the owner typed. */
  readonly title: string;
  /** The own shop is open (`IsVendorOpen`): the title is locked, OK and Reset vanish. */
  readonly open: boolean;
  /** The caption of the window (`TID_GAME_VENDOR_TITLE`, plus `TID_GAME_VENDOR_TITLE0` once the shop is open). */
  readonly windowTitle: string;
  /** The 20 slots (`MAX_VENDOR_REVISION`); `null` = empty. */
  readonly cells: readonly (VendorEntry | null)[];
}

export interface VendorActions {
  /**
   * The window finished building itself (`OnInitialUpdate` -> `ReloadItemList`): the OWNER's window registers the goods of the last session again
   * (`m_aSavedInven`) while the shop is closed. Not called for a visitor.
   */
  opened?(): void;
  /**
   * An item of the inventory dropped on the FREE slot `slot` of the own, closed shop: the feature refuses it (quest item, guild cloak, expired,
   * in use ...) or opens the price window. Returns whether the drop was taken (a refusal shows the forbid icon).
   */
  registerDrop(sc: Shortcut, slot: number): boolean;
  /** The right click on an item of the own shop, or dragging it back to the inventory (`SendUnregisterPVendorItem`). */
  unregister(slot: number): void;
  /** A double click, or dragging the goods of somebody else's shop to the inventory: the purchase window (`CWndVendorBuy`). */
  buy(slot: number): void;
  /** `WIDC_RESET`: unregisters every item of the shop and forgets the saved list. */
  reset(): void;
  /**
   * `WIDC_OK` with the typed title: the feature checks the penya, the title and the length and opens the confirmation box. Returns `true` when the
   * confirmation opened (the OK button hides until the box is answered: `restoreOk()` brings it back), `false` for a refusal (a box said why).
   */
  start(title: string): boolean;
  /** `WIDC_CANCEL` and the title-bar button with the shop OPEN: the "stop selling?" question (a closed shop just closes the window). */
  cancel(): void;
  /** `WIDC_CHAT`: opens or hides the chat window of the shop (the owner also opens the room: `SendOpenChattingRoom`). */
  toggleChat(): void;
  /** The window was destroyed: the feature closes the shop session (`SendPVendorClose`). */
  closed(): void;
}

export interface VendorPorts {
  readonly view: ViewSource<VendorView>;
  readonly actions: VendorActions;
  /**
   * `CWndVendor::Initialize`: the window refuses to open during a duel or while a trade, shop, bank or guild bank window is open (an applet
   * opened from the start menu asks). Absent = always.
   */
  canOpen?(): boolean;
}

// ---- APP_VENDOREX_SELL -----------------------------------------------------------------------------------------------------

/**
 * `CWndRegVend` (304 x 160): the quantity and unit price of an item put into the shop, typed on a number pad. OK clamps the quantity to the
 * stack and sends the digits of the price as text: the penya gate and the `INT_MAX` clamp are the feature's.
 */
export interface VendorSellRequest {
  /** The shop slot the item goes to. */
  readonly slot: number;
  /** The item (its icon and name for the window; the stack count starts in the quantity field). */
  readonly name: string;
  /** The stack the item holds (`m_nItemNum`): the quantity cannot pass it. */
  readonly stack: number;
  /**
   * OK with the quantity (at least 1, at most `stack`) and the price as typed; returns `true` when the registration was sent and the window may
   * close, `false` when a box refused it (the window stays: the digits are still there to change).
   */
  register(slot: number, quantity: number, priceText: string): boolean;
  closed?(): void;
}

// ---- APP_VENDOREX_BUY ------------------------------------------------------------------------------------------------------

/** `CWndVendorBuy` (304 x 160): the quantity of a purchase, typed on the number pad (it starts at everything that is for sale). */
export interface VendorBuyRequest {
  readonly slot: number;
  readonly name: string;
  /** What is for sale (`GetExtra()`): the quantity cannot pass it. */
  readonly count: number;
  /** The unit price (`m_nCost`). */
  readonly price: number;
  /**
   * OK with the quantity (at least 1): the feature checks the wallet (`TID_GAME_LACKMONEY` as a box over the window) and sends. Returns `true`
   * when the purchase was sent and the window may close.
   */
  confirm(slot: number, quantity: number): boolean;
  closed?(): void;
}

// ---- APP_VENDOREX_CHAT -----------------------------------------------------------------------------------------------------

/** What the chat room window shows (`CWndVendorMessage`: the talk page, the info page of the owner and the member list). */
export interface VendorChatView {
  /** Bumps with every change. */
  readonly version: number;
  /** `m_nIsOwner`: the owner has the second page "Info" with the joins, leaves and sales. */
  readonly owner: boolean;
  /** The names in the room (`WIDC_LISTBOX1`), in join order. */
  readonly members: readonly string[];
  /** The talk page: markup lines in order (`#cffff0000name :#nc\n  text\n`). */
  readonly talk: readonly string[];
  /** The info page of the owner: markup lines in order. */
  readonly info: readonly string[];
}

export interface VendorChatActions {
  /** Enter in the input edit, or the Send button: one line (`SendChatting`). The window empties the edit. */
  send(text: string): void;
  /** The window was closed by its own button: the owner closes the room (`SendCloseChattingRoom`). */
  closedByUser(): void;
  /** The window is gone (any reason). */
  closed(): void;
}

export interface VendorChatPorts {
  readonly view: ViewSource<VendorChatView>;
  readonly actions: VendorChatActions;
}
