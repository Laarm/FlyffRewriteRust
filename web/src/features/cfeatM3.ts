// The feature modules of the cfeat-m3 task (WP-CFEAT-09..12, -51) as handler modules of `app/features.ts` (`FEATURE_MODULES`):
// the inventory (the item store and its window), the status window and target frame, the character window and stat allocation,
// the death / revival dialog and the motions. Registered after the ui-m3 modules (the inventory plugs into the taskbar).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { characterModule } from './character';
import { statusModule } from './hud/status/statusModule';
import { inventoryModule } from './inventory';
import { motionsModule } from './motions';
import { revivalModule } from './pvp';

export const CFEAT_M3_FEATURE_MODULES: readonly HandlerModule[] = [
  inventoryModule,
  statusModule,
  characterModule,
  revivalModule,
  motionsModule,
].map(toHandlerModule);
