// The feature modules of the party-client task (WP-CFEAT-34, -35) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the party
// messages, windows, skills, finder and navigator marks, with the player-data cache. Registered after the NPC, item, action-slot and PvP groups:
// the player menu entry, the dispatch window of the party-summon scroll, the party-skill port of the taskbar and the buff strip that it plugs
// into exist by then.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { partyModule } from './party';

export const PARTY_M5_FEATURE_MODULES: readonly HandlerModule[] = [partyModule].map(
  toHandlerModule,
);
