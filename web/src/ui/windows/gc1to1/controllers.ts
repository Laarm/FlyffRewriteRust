// The 1:1 guild combat windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-55): one layout becomes `built`, 676 (`APP_GUILDCOMBAT_1TO1_OFFERSTATE`). The bid window (675) and the
// other 1:1 windows are the siege's shells in mode 1; the line-up (672) is `ui/windows/secretroom`.
import type { WndCtor } from '../../catalog';
import { GC1TO1_LAZY } from './lazy';

export const GC1TO1_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_GUILDCOMBAT_1TO1_OFFERSTATE: () => GC1TO1_LAZY.WndGuildCombat1to1OfferState.load(),
};
