// The feature modules of the ui-messenger task (WP-CFEAT-39) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the `messenger` feature. Registered after the party
// group (the friend menu invites to the party, the player-data cache is registered by the party feature) and the NPC menu table, the taskbar, the chat and the mail feature.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { messengerModule } from './messenger';

export const MESSENGER_M6_FEATURE_MODULES: readonly HandlerModule[] = [messengerModule].map(
  toHandlerModule,
);
