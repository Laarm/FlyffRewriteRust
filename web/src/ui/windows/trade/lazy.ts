// The trade request windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports these shells dynamically. They are opened with
// request objects (`ui.trade.openRequest` / `openFinalConfirm`) when a player asks to trade and when the partner confirmed. The trade window itself
// (`APP_TRADE`) is an applet: the applet registry builds it synchronously, so it is not here.
import { LazyWnd } from '../../wnd/lazy';
import type * as ConfirmTrade from './WndConfirmTrade';
import type * as TradeConfirm from './WndTradeConfirm';

export const TRADE_LAZY = {
  WndConfirmTrade: new LazyWnd<typeof ConfirmTrade.WndConfirmTrade>(
    'WndConfirmTrade',
    async () => (await import('./WndConfirmTrade')).WndConfirmTrade,
  ),
  WndTradeConfirm: new LazyWnd<typeof TradeConfirm.WndTradeConfirm>(
    'WndTradeConfirm',
    async () => (await import('./WndTradeConfirm')).WndTradeConfirm,
  ),
} as const;
