// The feature modules of the ui-guild task (WP-CFEAT-37, -38) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the `guild` feature. Registered after the messenger group
// (the guild page of the messenger reads the roster of this feature, the presence overlay is the messenger's) and the party, inventory, NPC menu, tooltip and bank groups it reads;
// before the siege and relation groups.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { guildModule } from './guild';

export const GUILD_M6_FEATURE_MODULES: readonly HandlerModule[] = [guildModule].map(
  toHandlerModule,
);
