// `TradeWindowsService` (`ui.trade`, Plan/42 §19.4 F; WP-UI-26): the player-trade windows.
//
//   ui.trade.setPorts({ view, actions })       45 WP-CFEAT-21: the read model and the actions of the trade window
//   (applet)   APP_TRADE opens through the applet registry (the feature opens it with `wm.open( APP_TRADE )` when `TradeNtf` arrives)
//   ui.trade.openRequest(request)              APP_CONFIRM_TRADE: "<name> asks to trade" (a second one replaces the first)
//   ui.trade.openFinalConfirm(request)         APP_TRADE_CONFIRM: "really trade?" (the feature opens it on `TradeLastConfirm`)
//
// The applet factory reads the ports at the moment a window is created, so ports installed later reach windows opened later.

import { APP_CONFIRM_TRADE, APP_TRADE, APP_TRADE_CONFIRM } from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import {
  constantView,
  type TradeFinalConfirm,
  type TradePorts,
  type TradeRequestWindow,
  type TradeView,
} from './ports';
import { openLazy } from '../../wnd/lazy';
import { TRADE_LAZY } from './lazy';
import type { WndConfirmTrade } from './WndConfirmTrade';
import { WndTrade } from './WndTrade';
import type { WndTradeConfirm } from './WndTradeConfirm';

export * from './ports';
export { CONFIRM_TRADE_Y } from './constants';
export type { WndConfirmTrade } from './WndConfirmTrade';
export {
  WndTrade,
  GRID_ME_RECT,
  GRID_YOU_RECT,
  TRADE_EXCLUDES,
  WIDC_GRID_ME,
  WIDC_GRID_YOU,
} from './WndTrade';
export type { WndTradeConfirm } from './WndTradeConfirm';

export interface TradeWindowDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

const EMPTY_SIDE = { name: '', gold: 0, cells: [], ok: false } as const;
/** The view before 45 installs the real one: no session. */
export const EMPTY_TRADE_VIEW: TradeView = {
  version: 0,
  active: false,
  you: EMPTY_SIDE,
  me: EMPTY_SIDE,
};

/** The ports before 45 installs the real ones: no session, actions that do nothing. */
export const NULL_TRADE_PORTS: TradePorts = {
  view: constantView(EMPTY_TRADE_VIEW),
  actions: {
    drop: () => false,
    pull: () => undefined,
    ok: () => undefined,
    cancel: () => undefined,
    closed: () => undefined,
  },
};

export class TradeWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;
  #ports: TradePorts = NULL_TRADE_PORTS;

  constructor(deps: TradeWindowDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
    deps.wm.applets.register({ app: APP_TRADE, factory: () => this.createTrade() });
  }

  /** Installs the ports of 45. */
  setPorts(ports: TradePorts): void {
    this.#ports = ports;
  }

  get ports(): TradePorts {
    return this.#ports;
  }

  createTrade(): WndTrade {
    return new WndTrade({
      view: this.#ports.view,
      actions: this.#ports.actions,
      tooltip: this.#tooltip,
      getText: this.#getText,
    });
  }

  /** The open trade window, if any. */
  trade(): WndTrade | null {
    const w = this.#wm.get(APP_TRADE);
    return w instanceof WndTrade ? w : null;
  }

  /** `CreateApplet( APP_TRADE )` (`OnTrade`): the window of the session; an open one is kept. */
  openTrade(): WndTrade | null {
    this.#wm.flushDestroyed();
    return this.#wm.open<WndTrade>(APP_TRADE);
  }

  /**
   * `APP_CONFIRM_TRADE`; a request window that is open is replaced (`SAFE_DELETE( m_pWndConfirmTrade )` precedes the new one). `null` while the
   * chunk of the shell is still loading: the window then opens by itself.
   */
  openRequest(request: TradeRequestWindow): WndConfirmTrade | null {
    return openLazy(
      this.#wm,
      APP_CONFIRM_TRADE,
      TRADE_LAZY.WndConfirmTrade,
      (C) => new C(request),
      () => {
        this.closeRequest();
      },
    );
  }

  /** The request window is open and not on its way out (the auto-decline rule of 45 §14.1 asks). */
  isRequestOpen(): boolean {
    const w = this.#wm.get(APP_CONFIRM_TRADE);
    return w !== null && !this.#wm.isDestroying(w);
  }

  /** The request window goes without an answer (`DelObj` of the requester, a duel begins, the trade started). */
  closeRequest(): void {
    const old = this.#wm.get(APP_CONFIRM_TRADE);
    if (old) {
      this.#wm.destroy(old);
      this.#wm.flushDestroyed();
    }
  }

  request(): WndConfirmTrade | null {
    const w = this.#wm.get(APP_CONFIRM_TRADE);
    const C = TRADE_LAZY.WndConfirmTrade.ctor;
    return C && w instanceof C ? w : null;
  }

  /** `APP_TRADE_CONFIRM` (`OnTradelastConfirm`: `m_pWndTradeConfirm = new CWndTradeConfirm`). */
  openFinalConfirm(request: TradeFinalConfirm): WndTradeConfirm | null {
    return openLazy(
      this.#wm,
      APP_TRADE_CONFIRM,
      TRADE_LAZY.WndTradeConfirm,
      (C) => new C(request),
      () => {
        this.closeFinalConfirm();
      },
    );
  }

  finalConfirm(): WndTradeConfirm | null {
    const w = this.#wm.get(APP_TRADE_CONFIRM);
    const C = TRADE_LAZY.WndTradeConfirm.ctor;
    return C && w instanceof C ? w : null;
  }

  closeFinalConfirm(): void {
    const old = this.#wm.get(APP_TRADE_CONFIRM);
    if (old) {
      this.#wm.destroy(old);
      this.#wm.flushDestroyed();
    }
  }
}
