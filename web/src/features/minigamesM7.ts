// The feature groups of the ui-minigames task (WP-CFEAT-45) as handler modules of `app/features.ts` (`FEATURE_MODULES`):
//   `events.minigames`  the NPC mini-games, the 1:1 guild combat, the attendance and coupon notices under the one `events` feature context (ids are unique; the `events` feature registers once per
//                       context, `registerEvents` is idempotent, so this group adds its own registrations on the same context). Registered last of the M7 event groups: it reads `menuOf`, `siegeOf`,
//                       `guildOf`, `inventoryOf`, `messengerOf` and `playerDataOf`, all registered by earlier groups.
//   `tickets`           the client ticket rows for the item dispatch (R24) on the `itemuse` feature context; it needs `inventoryOf`.
// No `FeatureId` is added to the union.

import type { HandlerModule } from '../app/features';
import { createFeatureContext } from './common/module';
import { registerEvents } from './events';
import { registerAttendance } from './events/attendance';
import { registerGc1to1 } from './events/gc1to1';
import { registerMiniGames } from './events/minigame';
import { registerTickets } from './tickets';

export const minigamesModule: HandlerModule = {
  id: 'events.minigames',
  register(ctx) {
    const fc = createFeatureContext(ctx, 'events');
    registerEvents(fc);
    registerMiniGames(fc);
    registerGc1to1(fc);
    registerAttendance(fc);
  },
};

export const ticketsModule: HandlerModule = {
  id: 'tickets',
  register(ctx) {
    registerTickets(createFeatureContext(ctx, 'itemuse'));
  },
};

export const MINIGAMES_M7_FEATURE_MODULES: readonly HandlerModule[] = [
  minigamesModule,
  ticketsModule,
];
