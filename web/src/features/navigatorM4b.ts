// The feature modules of the ui-nav task (WP-CFEAT-28, -29, -35) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the navigator
// minimap with the world-map overlay and the teleporter mode, the TELE16 window, the inspection of another player and the collecting panel.
// Registered after the NPC, item, party and inventory groups (the menu entries `MMI_TELEPORTER` / `MMI_QUERYEQUIP`, the party marks, the
// inventory store and the tooltip owner exist by then).

import type { HandlerModule } from '../app/features';
import { createFeatureContext, type FeatureModule } from './common/module';
import { collectingModule } from './inspect/collecting';
import { inspectModule } from './inspect/module';
import { navigatorModule } from './navigator/module';
import { tele16Module } from './tele16/module';

/** A feature module under its own handler-module id (the registry refuses two modules with one id; the feature id stays the module's). */
function named(id: string, m: FeatureModule): HandlerModule {
  return {
    id,
    register(ctx) {
      m.register(createFeatureContext(ctx, m.id));
    },
  };
}

export const NAV_M4B_FEATURE_MODULES: readonly HandlerModule[] = [
  named('map.navigator', navigatorModule),
  named('map.tele16', tele16Module),
  named('inventory.inspect', inspectModule),
  named('inventory.collecting', collectingModule),
];
