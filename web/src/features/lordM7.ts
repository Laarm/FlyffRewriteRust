// The feature module of the ui-lord task (WP-CFEAT-43) as a handler module of `app/features.ts` (`FEATURE_MODULES`): the lord, the election, the lord events and the lord skills under the handler
// module id `events.lord` (ids are unique; the `events` feature registers once per context, `registerEvents` is idempotent, so this group adds its own registrations on the same `events` feature
// context). Registered after the M6 groups and ui-quest's: it reads `menuOf`, `taskbarOf`, `playerDataOf` and `eventOf`, all registered by earlier groups.

import type { HandlerModule } from '../app/features';
import { createFeatureContext } from './common/module';
import { registerEvents } from './events';
import { registerLord } from './events/lord';

export const lordModule: HandlerModule = {
  id: 'events.lord',
  register(ctx) {
    const fc = createFeatureContext(ctx, 'events');
    registerEvents(fc);
    registerLord(fc);
  },
};

export const LORD_M7_FEATURE_MODULES: readonly HandlerModule[] = [lordModule];
