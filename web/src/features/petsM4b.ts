// The feature modules of the ui-pets task (WP-CFEAT-27) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the `pet` feature. Registered after the
// inventory (the dispatch windows and the feed), the NPC menu table and the item-use group (the eat pet name of the vis-key question).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { petsModule } from './pets';

export const PETS_M4B_FEATURE_MODULES: readonly HandlerModule[] = [petsModule].map(toHandlerModule);
