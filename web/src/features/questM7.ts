// The feature module of the ui-quest task (WP-CFEAT-42) as a handler module of `app/features.ts` (`FEATURE_MODULES`): the `quest` feature. The first group of milestone 7, registered after the M6
// groups: it reads `dialogOf`, `navigatorOf`, `partyOf`, `guildOf`, `inventoryOf`, `skillTreeOf`, `petsOf` and `relationOf` at call time. Later M7 client tasks append their groups after it.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { questModule } from './quest';

export const QUEST_M7_FEATURE_MODULES: readonly HandlerModule[] = [questModule].map(
  toHandlerModule,
);
