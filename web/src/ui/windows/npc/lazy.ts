// The NPC windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically. The windows are opened with request
// objects (`ui.npc.open*`), never by the taskbar, so none of them has to exist before the player talks to an NPC.
import { LazyWnd } from '../../wnd/lazy';
import type * as BeautyConfirm from './WndBeautyConfirm';
import type * as BeautyShop from './WndBeautyShop';
import type * as ConfirmBuy from './WndConfirmBuy';
import type * as ConfirmSell from './WndConfirmSell';
import type * as DialogEx from './WndDialogEx';
import type * as FaceShop from './WndFaceShop';
import type * as QuickSell from './WndQuickSell';
import type * as Shop from './WndShop';
import type * as Warning from './WndWarning';

export const NPC_LAZY = {
  WndDialogEx: new LazyWnd<typeof DialogEx.WndDialogEx>(
    'WndDialogEx',
    async () => (await import('./WndDialogEx')).WndDialogEx,
  ),
  WndShop: new LazyWnd<typeof Shop.WndShop>(
    'WndShop',
    async () => (await import('./WndShop')).WndShop,
  ),
  WndConfirmBuy: new LazyWnd<typeof ConfirmBuy.WndConfirmBuy>(
    'WndConfirmBuy',
    async () => (await import('./WndConfirmBuy')).WndConfirmBuy,
  ),
  WndConfirmSell: new LazyWnd<typeof ConfirmSell.WndConfirmSell>(
    'WndConfirmSell',
    async () => (await import('./WndConfirmSell')).WndConfirmSell,
  ),
  WndWarning: new LazyWnd<typeof Warning.WndWarning>(
    'WndWarning',
    async () => (await import('./WndWarning')).WndWarning,
  ),
  WndQuickSell: new LazyWnd<typeof QuickSell.WndQuickSell>(
    'WndQuickSell',
    async () => (await import('./WndQuickSell')).WndQuickSell,
  ),
  WndBeautyShop: new LazyWnd<typeof BeautyShop.WndBeautyShop>(
    'WndBeautyShop',
    async () => (await import('./WndBeautyShop')).WndBeautyShop,
  ),
  WndFaceShop: new LazyWnd<typeof FaceShop.WndFaceShop>(
    'WndFaceShop',
    async () => (await import('./WndFaceShop')).WndFaceShop,
  ),
  WndBeautyConfirm: new LazyWnd<typeof BeautyConfirm.WndBeautyConfirm>(
    'WndBeautyConfirm',
    async () => (await import('./WndBeautyConfirm')).WndBeautyConfirm,
  ),
} as const;
