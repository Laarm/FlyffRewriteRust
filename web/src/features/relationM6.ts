// The feature module of the ui-couple-honor task (WP-CFEAT-40) as a handler module of `app/features.ts` (`FEATURE_MODULES`): the `relation` feature. Registered after the messenger
// group (the campus points of its header and the player-data cache), the motions, the NPC menu table, the party, inventory, character and item-use modules it reads.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { relationModule } from './relation';

export const RELATION_M6_FEATURE_MODULES: readonly HandlerModule[] = [relationModule].map(
  toHandlerModule,
);
