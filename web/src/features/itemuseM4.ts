// The feature modules of the ui-items-misc task (WP-CFEAT-15, WP-UI-35) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the
// item-specific use windows (`itemuse`) and the appearance flows (`appearance`: the beauty coupons, the sex-change scroll, the item transy, the
// face window). Registered after the cfeat-items group (they read the inventory and the tooltip numbers) and the NPC group (the beauty shops).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { appearanceModule } from './appearance';
import { itemuseModule } from './itemuse';

export const ITEMUSE_M4_FEATURE_MODULES: readonly HandlerModule[] = [
  itemuseModule,
  appearanceModule,
].map(toHandlerModule);
