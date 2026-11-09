// The exchange window of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-36 shell): the lazy class of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { EXCHANGE_LAZY } from './lazy';

export const EXCHANGE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_DIALOG_EVENT: () => EXCHANGE_LAZY.WndDialogEvent.load(),
};
