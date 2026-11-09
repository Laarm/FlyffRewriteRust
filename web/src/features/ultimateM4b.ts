// The feature modules of the ui-upgrade-b task (WP-CFEAT-26) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the ultimate-weapon
// windows and the NPC item-exchange list. Registered after the NPC group (the menu feature takes their actions and the exchange fallback) and the
// item groups (the inventory feeds the windows and the dispatch asks them).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { exchangeModule } from './exchange';
import { ultimateModule } from './ultimate';

export const ULTIMATE_M4B_FEATURE_MODULES: readonly HandlerModule[] = [
  ultimateModule,
  exchangeModule,
].map(toHandlerModule);
