// The feature group of the ui-rainbow task (WP-CFEAT-44): the rainbow race with its seven mini-games and the quiz event. ONE handler module (`events.rainbow`) on the feature context of `events`, like the other M7 event groups:
// `registerEvents` is idempotent, the two registrations read the events store, the NPC menu, the player-data cache and the HUD, which all register earlier in `FEATURE_MODULES`. Registered after the housing group.

import type { HandlerModule } from '../app/features';
import { createFeatureContext } from './common/module';
import { registerEvents } from './events';
import { registerQuiz } from './events/quiz';
import { registerRainbow } from './events/rainbow';

export const eventsRainbowModule: HandlerModule = {
  id: 'events.rainbow',
  register(ctx) {
    const fc = createFeatureContext(ctx, 'events');
    registerEvents(fc);
    registerRainbow(fc);
    registerQuiz(fc);
  },
};

export const RAINBOW_M7_FEATURE_MODULES: readonly HandlerModule[] = [eventsRainbowModule];
