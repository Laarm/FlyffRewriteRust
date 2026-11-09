// The feature group of the ui-secret-tax task (WP-CFEAT-52) as ONE handler module of `app/features.ts` (`FEATURE_MODULES`): the tax tables and rate windows, the secret room and the Heaven Tower under the
// handler module id `events.secretRoomTax` (ids are unique; the `events` feature registers once per context, `registerEvents` is idempotent, so this group adds its own registrations on the same `events`
// feature context). Registered after the lord group: it reads `guildOf`, `playerDataOf`, `menuOf`, `tooltipOf` and `ui.quest` at call time, all registered by earlier groups.

import type { HandlerModule } from '../app/features';
import { createFeatureContext } from './common/module';
import { registerEvents } from './events';
import { registerHeavenTower } from './events/heavenTower';
import { registerSecretRoom } from './events/secretRoom';
import { registerTax } from './events/tax';

export const secretRoomTaxModule: HandlerModule = {
  id: 'events.secretRoomTax',
  register(ctx) {
    const fc = createFeatureContext(ctx, 'events');
    registerEvents(fc);
    // the tax first: the secret room's menu entry 89 and the shop read it
    registerTax(fc);
    registerSecretRoom(fc);
    registerHeavenTower(fc);
  },
};

export const SECRET_TAX_M7_FEATURE_MODULES: readonly HandlerModule[] = [secretRoomTaxModule];
