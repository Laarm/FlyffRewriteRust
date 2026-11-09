// The bank windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-25): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { BANK_LAZY } from './lazy';

export const BANK_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_COMMON_BANK: () => BANK_LAZY.WndBank.load(),
  APP_CONFIRM_BANK: () => BANK_LAZY.WndConfirmBank.load(),
  APP_BANK_PASSWORD: () => BANK_LAZY.WndBankPassword.load(),
};
