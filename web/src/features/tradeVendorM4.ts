// The feature modules of the ui-trade-vendor task (WP-CFEAT-21, -22) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the player trade
// and the personal shop with its chat room. Registered after the NPC, item and party groups: the menu actions (`MMI_TRADE` of a player), the
// inventory (reservations, the held-back penya, the drop route of the shop window) and the taskbar slot of the trade exist by then.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { tradeModule } from './trade';
import { vendorModule } from './vendor';

export const TRADE_VENDOR_M4_FEATURE_MODULES: readonly HandlerModule[] = [
  tradeModule,
  vendorModule,
].map(toHandlerModule);
