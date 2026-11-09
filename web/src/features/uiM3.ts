// The feature modules of the ui-m3 task as handler modules of `app/features.ts` (`FEATURE_MODULES`): the taskbar ingest and its
// ports (WP-UI-19). Registered after the hud-chat modules (the taskbar executes chat macros through the chat feature).

import type { HandlerModule } from '../app/features';
import { toHandlerModule } from './common/module';
import { taskbarModule } from './taskbar';

export const UI_M3_FEATURE_MODULES: readonly HandlerModule[] = [taskbarModule].map(toHandlerModule);
