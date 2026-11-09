// The feature modules of the skill-client task (WP-CFEAT-32) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the action slot
// with the skill source of the taskbar. Registered after the taskbar module (it fills the taskbar's ports); the feature id is `skills`
// (the skill-tree module of the skill-windows task has its own handler-module id), so this one gets an id of its own.

import type { HandlerModule } from '../app/features';
import { createFeatureContext, type FeatureModule } from './common/module';
import { actionSlotModule } from './actionslot';

function named(id: string, m: FeatureModule): HandlerModule {
  return {
    id,
    register(ctx) {
      m.register(createFeatureContext(ctx, m.id));
    },
  };
}

export const ACTIONSLOT_M5_FEATURE_MODULES: readonly HandlerModule[] = [
  named('skills.actionslot', actionSlotModule),
];
