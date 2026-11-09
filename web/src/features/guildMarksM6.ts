// The feature module of the render-guild task (WP-REND-26, WP-INPUT-16) as a handler module of `app/features.ts` (`FEATURE_MODULES`): the `guildmarks` feature.
// Registered last, after the messenger, guild, siege and relation groups: it reads the siege slice of the events store and the guild briefs of the movers.

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { guildMarksModule } from './guildmarks';

export const GUILD_MARKS_M6_FEATURE_MODULES: readonly HandlerModule[] = [guildMarksModule].map(
  toHandlerModule,
);
