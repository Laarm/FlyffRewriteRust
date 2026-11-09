// The feature modules of the skill-windows task (WP-CFEAT-31, -33) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the skill
// tree (editing, reset, the legend skill-up, the skill tooltip) and the job change windows (the fast job change, the class-change scroll).
// Registered after the NPC and item modules (the NPC menu action, the dispatch windows and the inventory feed they plug into exist by then).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { jobChangeModule } from './jobchange';
import { skillTreeModule } from './skills/tree';

export const SKILL_WINDOWS_M5_FEATURE_MODULES: readonly HandlerModule[] = [
  skillTreeModule,
  jobChangeModule,
].map(toHandlerModule);
