// The feature modules of the hud-chat task as handler modules of `app/features.ts` (`FEATURE_MODULES`, registered at boot step B5,
// before the WebSocket opens): the HUD messages, the chat routing and input, the server texts and the browser-side commands.
// The order is the order of registration: `hud` first (the chat router's CLIENT target), then `chat`, then `notice` (it
// registers the local commands the chat input pipeline consults).

import type { HandlerModule } from '../app/features';
import { chatModule } from './chat';
import { toHandlerModule } from './common/module';
import { hudModule } from './hud';
import { textsModule } from './texts';

export const HUD_CHAT_FEATURE_MODULES: readonly HandlerModule[] = [
  hudModule,
  chatModule,
  textsModule,
].map(toHandlerModule);
