// The feature module of the ui-siege task (WP-CFEAT-41) as a handler module of `app/features.ts` (`FEATURE_MODULES`): the event flags and the HUD timer (`events`) and the guild siege
// (`siege`) under the one feature id `events` (a handler module id is unique, and the M7 event tasks join the same module). Registered after the messenger and guild groups (the siege
// reads `guildOf( ctx )` and the player-data cache), before the relation group.

import type { HandlerModule } from '../app/features';
import { type FeatureModule, toHandlerModule } from './common/module';
import { registerEvents } from './events';
import { registerSiege } from './events/siege';

export const eventsSiegeModule: FeatureModule = {
  id: 'events',
  milestone: 6,
  register(fc) {
    registerEvents(fc);
    registerSiege(fc);
  },
};

export const SIEGE_M6_FEATURE_MODULES: readonly HandlerModule[] = [eventsSiegeModule].map(
  toHandlerModule,
);
