// The feature modules of the ui-npc-shop task (WP-CFEAT-18, -19) as handler modules of `app/features.ts` (`FEATURE_MODULES`):
// the NPC click menus, the dialog window and the NPC shop. Registered after the cfeat-m3 modules (the shop plugs into the
// inventory's drop handling and the bag).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { npcModule } from './npc';

export const NPC_M4_FEATURE_MODULES: readonly HandlerModule[] = [npcModule].map(toHandlerModule);
