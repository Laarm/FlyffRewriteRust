// The feature modules of the duel-ui task (WP-CFEAT-36) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the duel windows and
// messages, the resurrection offer, the PvP option and the DPS meter. Registered after the NPC modules (the duel entries plug into the
// player menu). The first module of `features/pvp` is the revival of the cfeat-m3 group with the same feature id, so the handler-module ids
// here are their own (`pvp.duel`, ...); each module still gets the `FeatureContext` of its feature id.

import type { HandlerModule } from '../app/features';
import { createFeatureContext, type FeatureModule } from './common/module';
import { dpsModule } from './hud/dps/dpsModule';
import { duelModule } from './pvp/duel/duelModule';
import { pvpOptionModule } from './pvp/option/pvpOption';
import { resurrectionModule } from './pvp/resurrection/resurrectionModule';

/** A feature module under its own handler-module id (the registry refuses two modules with one id). */
function named(id: string, m: FeatureModule): HandlerModule {
  return {
    id,
    register(ctx) {
      m.register(createFeatureContext(ctx, m.id));
    },
  };
}

export const PVP_M5_FEATURE_MODULES: readonly HandlerModule[] = [
  named('pvp.duel', duelModule),
  named('pvp.resurrection', resurrectionModule),
  named('pvp.option', pvpOptionModule),
  named('hud.dps', dpsModule),
];
