// The feature group of the ui-house-quiz task (WP-CFEAT-46, WP-INPUT-17) as a handler module of `app/features.ts` (`FEATURE_MODULES`): the personal mini room, the small guild house and the placement of their furniture. ONE handler
// module (`events.housing`) on the feature context of `events`, like the M6 siege group: `registerEvents` is idempotent, the three registrations read the events store, the NPC menu, the messenger and the guild feature, which
// all register earlier in `FEATURE_MODULES`. Registered after the quest and lord groups.

import type { HandlerModule } from '../app/features';
import { createFeatureContext } from './common/module';
import { registerEvents } from './events';
import { registerDeploy } from './events/deploy';
import { registerGuildHouse } from './events/guildHouse';
import { registerHousing } from './events/housing';

export const eventsHousingModule: HandlerModule = {
  id: 'events.housing',
  register(ctx) {
    const fc = createFeatureContext(ctx, 'events');
    registerEvents(fc);
    registerHousing(fc);
    registerGuildHouse(fc);
    registerDeploy(fc);
  },
};

export const HOUSE_M7_FEATURE_MODULES: readonly HandlerModule[] = [eventsHousingModule];
