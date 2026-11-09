// The duel and resurrection windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-41): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { DUEL_LAZY } from './lazy';

export const DUEL_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_DUEL_CONFIRM: () => DUEL_LAZY.WndDuelConfirm.load(),
  APP_DUEL_RESULT: () => DUEL_LAZY.WndDuelResult.load(),
  APP_RESURRECTION: () => DUEL_LAZY.WndResurrectionConfirm.load(),
};
