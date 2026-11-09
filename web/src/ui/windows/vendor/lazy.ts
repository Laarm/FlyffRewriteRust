// The shop dialogs in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. They are opened with request
// objects (`ui.vendor.openSell` / `openBuy` / `openChat`) while a shop is set up or visited. The shop window itself (`APP_VENDOR_REVISION`) is an
// applet: the applet registry builds it synchronously, so it is not here.
import { LazyWnd } from '../../wnd/lazy';
import type * as VendorBuy from './WndVendorBuy';
import type * as VendorChat from './WndVendorChat';
import type * as VendorSell from './WndVendorSell';

export const VENDOR_LAZY = {
  WndVendorSell: new LazyWnd<typeof VendorSell.WndVendorSell>(
    'WndVendorSell',
    async () => (await import('./WndVendorSell')).WndVendorSell,
  ),
  WndVendorBuy: new LazyWnd<typeof VendorBuy.WndVendorBuy>(
    'WndVendorBuy',
    async () => (await import('./WndVendorBuy')).WndVendorBuy,
  ),
  WndVendorChat: new LazyWnd<typeof VendorChat.WndVendorChat>(
    'WndVendorChat',
    async () => (await import('./WndVendorChat')).WndVendorChat,
  ),
} as const;
