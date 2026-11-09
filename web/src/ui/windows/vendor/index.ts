// `VendorWindowsService` (`ui.vendor`, Plan/42 §19.4 F; WP-UI-26, WP-UI-37): the personal-shop windows.
//
//   ui.vendor.setPorts({ view, actions, canOpen })   45 WP-CFEAT-22: the read model and the actions of the shop window
//   (applet)   APP_VENDOR_REVISION opens through the applet registry (the start menu entry, the feature's `openOwn` / the `PVendorItem` answer)
//   ui.vendor.openSell(request)                      APP_VENDOREX_SELL: the quantity and unit price of an item put into the shop
//   ui.vendor.openBuy(request)                       APP_VENDOREX_BUY: the quantity of a purchase
//   ui.vendor.toggleChat(ports)                      APP_VENDOREX_CHAT: opens the room window, or hides / shows the open one (the Chat button)
//
// The applet factory reads the ports at the moment a window is created, so ports installed later reach windows opened later. Each `open*` replaces
// the window of the same kind that is open (`SAFE_DELETE( m_pWndRegVend ); m_pWndRegVend = new CWndRegVend`).

import {
  APP_VENDOR_REVISION,
  APP_VENDOREX_BUY,
  APP_VENDOREX_CHAT,
  APP_VENDOREX_SELL,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { VENDOR_LAZY } from './lazy';
import {
  constantView,
  type VendorBuyRequest,
  type VendorChatPorts,
  type VendorPorts,
  type VendorSellRequest,
  type VendorView,
} from './ports';
import { WndVendor } from './WndVendor';
import type { WndVendorBuy } from './WndVendorBuy';
import type { WndVendorChat } from './WndVendorChat';
import type { WndVendorSell } from './WndVendorSell';

export * from './ports';
export * from './keypad';
export { PAD_NUMBER_IDS } from './padIds';
export { WndVendor, VENDOR_GRID_RECT, VENDOR_TITLE_MAX, WIDC_VENDOR_GRID } from './WndVendor';
export { CHAT_LINE_MAX, FOCUS_MARKER, FOCUS_MARKER_SIZE, PRICE_MAX } from './constants';
export type { WndVendorBuy } from './WndVendorBuy';
export type { WndVendorChat } from './WndVendorChat';
export type { WndVendorSell } from './WndVendorSell';

export interface VendorWindowDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

/** The view before 45 installs the real one: an empty own shop. */
export const EMPTY_VENDOR_VIEW: VendorView = {
  version: 0,
  owner: true,
  title: '',
  open: false,
  windowTitle: '',
  cells: [],
};

/** The ports before 45 installs the real ones: an empty shop, actions that do nothing. */
export const NULL_VENDOR_PORTS: VendorPorts = {
  view: constantView(EMPTY_VENDOR_VIEW),
  actions: {
    registerDrop: () => false,
    unregister: () => undefined,
    buy: () => undefined,
    reset: () => undefined,
    start: () => false,
    cancel: () => undefined,
    toggleChat: () => undefined,
    closed: () => undefined,
  },
};

export class VendorWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;
  #ports: VendorPorts = NULL_VENDOR_PORTS;

  constructor(deps: VendorWindowDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
    deps.wm.applets.register({ app: APP_VENDOR_REVISION, factory: () => this.createVendor() });
  }

  /** Installs the ports of 45. */
  setPorts(ports: VendorPorts): void {
    this.#ports = ports;
  }

  get ports(): VendorPorts {
    return this.#ports;
  }

  createVendor(): WndVendor {
    return new WndVendor({
      view: this.#ports.view,
      actions: this.#ports.actions,
      tooltip: this.#tooltip,
      canOpen: () => this.#ports.canOpen?.() ?? true,
    });
  }

  /** The open shop window (not one on its way out), if any. */
  vendor(): WndVendor | null {
    const w = this.#wm.get(APP_VENDOR_REVISION);
    return w instanceof WndVendor && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `CreateApplet( APP_VENDOR_REVISION )`: an open window is kept and focused; `null` when the window refuses to open. */
  openVendor(): WndVendor | null {
    this.#wm.flushDestroyed();
    return this.#wm.open<WndVendor>(APP_VENDOR_REVISION);
  }

  // ---- the dialogs of the shop window ------------------------------------------------------------------------------------------

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (old) {
      this.#wm.destroy(old);
      this.#wm.flushDestroyed();
    }
  }

  /** Opens the window of a lazy class (`null` while its chunk is still loading: the window then opens by itself). */
  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  /** The open window of a lazy class, if any. */
  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>): InstanceType<C> | null {
    const w = this.#wm.get(app);
    const C = lazy.ctor;
    return C && w instanceof C ? (w as InstanceType<C>) : null;
  }

  openSell(request: VendorSellRequest): WndVendorSell | null {
    return this.#open(APP_VENDOREX_SELL, VENDOR_LAZY.WndVendorSell, (C) => new C(request));
  }

  openBuy(request: VendorBuyRequest): WndVendorBuy | null {
    return this.#open(APP_VENDOREX_BUY, VENDOR_LAZY.WndVendorBuy, (C) => new C(request));
  }

  sell(): WndVendorSell | null {
    return this.#get(APP_VENDOREX_SELL, VENDOR_LAZY.WndVendorSell);
  }

  buy(): WndVendorBuy | null {
    return this.#get(APP_VENDOREX_BUY, VENDOR_LAZY.WndVendorBuy);
  }

  closeSell(): void {
    this.#replace(APP_VENDOREX_SELL);
  }

  closeBuy(): void {
    this.#replace(APP_VENDOREX_BUY);
  }

  // ---- the chat room ---------------------------------------------------------------------------------------------------------

  chat(): WndVendorChat | null {
    return this.#get(APP_VENDOREX_CHAT, VENDOR_LAZY.WndVendorChat);
  }

  /** Opens the room window (an open one is kept). */
  openChat(ports: VendorChatPorts): WndVendorChat | null {
    const existing = this.chat();
    if (existing) return existing;
    return this.#open(
      APP_VENDOREX_CHAT,
      VENDOR_LAZY.WndVendorChat,
      (C) => new C({ ...ports, getText: this.#getText }),
    );
  }

  /** The Chat button: no window opens it; an open one is hidden, a hidden one shown (`SetVisible( !IsVisible() )`). */
  toggleChat(ports: VendorChatPorts): WndVendorChat | null {
    const existing = this.chat();
    if (!existing) return this.openChat(ports);
    existing.setVisible(!existing.isVisible());
    return existing;
  }

  closeChat(): void {
    this.#replace(APP_VENDOREX_CHAT);
  }
}

export type { WndNeuz };
