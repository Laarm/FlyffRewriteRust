// The feature modules of the cfeat-items task (WP-CFEAT-13, -14, -16, -17, -30) as handler modules of `app/features.ts`
// (`FEATURE_MODULES`): the item tooltip composer and the buff icons. The inventory double-click dispatch, the upgrade cursor and the
// item-use router are part of the inventory module (`features/inventory/dispatch`). Registered after the cfeat-m3 modules (they
// read the inventory store) and the NPC modules (the shop tells the tooltip about its window).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { buffsModule } from './buffs';
import { tooltipModule } from './tooltip';

export const ITEMS_M4_FEATURE_MODULES: readonly HandlerModule[] = [tooltipModule, buffsModule].map(
  toHandlerModule,
);
