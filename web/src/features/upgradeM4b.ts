// The feature modules of the ui-upgrade-a task (WP-CFEAT-24, WP-CFEAT-25) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the `upgrade` feature (safe
// upgrade windows, element windows, awakening windows). Registered after the inventory (the double-click feed), the NPC menu table and the item-use group.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { upgradeModule } from './upgrade';

export const UPGRADE_M4B_FEATURE_MODULES: readonly HandlerModule[] = [upgradeModule].map(
  toHandlerModule,
);
