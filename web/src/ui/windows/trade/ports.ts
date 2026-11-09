// Ports of the player-trade windows (Plan/42 §19.4 F, WP-UI-26; wired by `features/trade`, WP-CFEAT-21): the READ MODEL of the trade
// window (`TradeView`, observed through a `ViewSource`), its ACTIONS, and one request object per dialog (the incoming request, the final
// confirmation). The decisions behind the callbacks (which put is refused, the quantity boxes, the sends, the texts) are the feature's;
// the shells keep the control behaviour of the originals (`CWndTrade`, `CWndConfirmTrade`, `CWndTradeConfirm`,
// `src:_Interface/WndField.cpp:9150-9540`, `WndTradeCtrl.cpp`). Each action names the legacy send it covers (ref/99 App. B).

import type { Shortcut } from '../../dnd/shortcut';
import type { ItemView } from '../../tooltip/types';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** One offered item (`CItemElem` of `m_vtInfo`): everything the grid and the tooltip need. */
export interface TradeCell extends ItemView {
  /** The object id of the offered item (the owner's inventory entry). */
  readonly objId: number;
  readonly itemId: number;
  /** `Item/...` icon file; `null` draws nothing. */
  readonly icon: string | null;
  /** The OFFERED count (the stack count shown on a stackable item). */
  readonly count: number;
  /** `dwPackMax > 1`: the count is printed. */
  readonly showCount: boolean;
  /** An expired item is drawn red (`CItemElem::expired`). */
  readonly expired: boolean;
  /** The record for the tooltip composers (45's `InvItem`). */
  readonly item?: unknown;
}

/** One side of the window (`WIDC_STATIC1`/`WIDC_STATIC3` for "you", `WIDC_STATIC2`/`WIDC_STATIC4` for "me"). */
export interface TradeSideView {
  /** The name above the grid. */
  readonly name: string;
  /** The penya offered by this side (the running total). */
  readonly gold: number;
  /** The 25 slots (`MAX_TRADE`); `null` = empty. */
  readonly cells: readonly (TradeCell | null)[];
  /** The side pressed OK: its OK button is checked and disabled. */
  readonly ok: boolean;
}

export interface TradeView {
  /** Bumps with every change. */
  readonly version: number;
  /** A trade session exists (a window opened by hand, without one, closes itself). */
  readonly active: boolean;
  /** The partner (left grid, "you") and the own side (right grid, "me"). */
  readonly you: TradeSideView;
  readonly me: TradeSideView;
}

export interface TradeActions {
  /**
   * A shortcut dropped on the trade window: `slot` is the 0..24 slot under the pointer of the OWN grid (`null` = the partner's grid or the
   * frame). Item sources: the feature refuses (`TID_GAME_*` lines) or opens the quantity box for a stack, then sends `TradePutReq`
   * (`legacy-send: SendTradePut`); the gold coin (`data == 0`) opens the penya box (`SendTradePutGold`). Returns whether the drop was
   * taken (a refusal shows the forbid icon).
   */
  drop(sc: Shortcut, slot: number | null, onPartnerGrid: boolean): boolean;
  /**
   * A right click on an item of the own grid takes it back (`SendTradePull`; the original client never sent it, the protocol has it). The server
   * answers `TradePutError` when a side pressed OK already.
   */
  pull(slot: number): void;
  /** `WIDC_OK_I` (`SendTradeOk`). */
  ok(): void;
  /** `WIDC_CANCEL` and the title-bar close button (`SendTradeCancel`, mode 0). The window stays until `TradeCancelNtf` closes it. */
  cancel(): void;
  /** The window was destroyed (consent, cancel, `closeTransactional`): the feature drops its session state. */
  closed(): void;
}

export interface TradePorts {
  readonly view: ViewSource<TradeView>;
  readonly actions: TradeActions;
}

// ---- APP_CONFIRM_TRADE ----------------------------------------------------------------------------------------------------

/**
 * `CWndConfirmTrade` (272 x 160): "<name> asks to trade". OK sends `TradeReq{requester}` (`SendTrade`) and closes; Cancel and the close
 * button send `ConfirmTradeCancelReq{requester}` (`SendConfirmTradeCancel`) and close.
 */
export interface TradeRequestWindow {
  /** The requester's name (`TID_GAME_FROM` is applied by the shell). */
  readonly name: string;
  /** `TID_GAME_FROM` with the name filled in (the window prints it in `WIDC_STATIC2`). */
  readonly fromText: string;
  accept(): void;
  decline(): void;
}

// ---- APP_TRADE_CONFIRM ----------------------------------------------------------------------------------------------------

/**
 * `CWndTradeConfirm` (288 x 128): the final "really trade?" box. Yes sends `TradeConfirm` (`SendTradeConfirm`); No and the close button send
 * `TradeCancelReq{0}` (`SendTradeCancel`). After the own `TradeLastConfirmOk` the box shows the wait text and hides both buttons.
 */
export interface TradeFinalConfirm {
  /** `TID_DIAG_0083`. */
  readonly question: string;
  yes(): void;
  no(): void;
}
