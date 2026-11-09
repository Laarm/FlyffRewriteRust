// The bank windows in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports the shells dynamically.
import { LazyWnd } from '../../wnd/lazy';
import type * as Bank from './WndBank';
import type * as BankPassword from './WndBankPassword';
import type * as ConfirmBank from './WndConfirmBank';

export const BANK_LAZY = {
  WndBank: new LazyWnd<typeof Bank.WndBank>(
    'WndBank',
    async () => (await import('./WndBank')).WndBank,
  ),
  WndConfirmBank: new LazyWnd<typeof ConfirmBank.WndConfirmBank>(
    'WndConfirmBank',
    async () => (await import('./WndConfirmBank')).WndConfirmBank,
  ),
  WndBankPassword: new LazyWnd<typeof BankPassword.WndBankPassword>(
    'WndBankPassword',
    async () => (await import('./WndBankPassword')).WndBankPassword,
  ),
} as const;
