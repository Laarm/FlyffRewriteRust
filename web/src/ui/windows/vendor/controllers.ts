// The personal-shop windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-26, WP-UI-37): the dialogs are the lazy classes of `lazy.ts`;
// the shop window is an applet (built by the applet registry synchronously), so its class is a plain import.
import type { WndCtor } from '../../catalog';
import { VENDOR_LAZY } from './lazy';
import { WndVendor } from './WndVendor';

export const VENDOR_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_VENDOR_REVISION: () => Promise.resolve(WndVendor),
  APP_VENDOREX_SELL: () => VENDOR_LAZY.WndVendorSell.load(),
  APP_VENDOREX_BUY: () => VENDOR_LAZY.WndVendorBuy.load(),
  APP_VENDOREX_CHAT: () => VENDOR_LAZY.WndVendorChat.load(),
};
