// The trade windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-26): the two request windows are the lazy classes of `lazy.ts`; the
// trade window is an applet (built by the applet registry synchronously), so its class is a plain import.
import type { WndCtor } from '../../catalog';
import { TRADE_LAZY } from './lazy';
import { WndTrade } from './WndTrade';

export const TRADE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_TRADE: () => Promise.resolve(WndTrade),
  APP_CONFIRM_TRADE: () => TRADE_LAZY.WndConfirmTrade.load(),
  APP_TRADE_CONFIRM: () => TRADE_LAZY.WndTradeConfirm.load(),
};
