// `web/src/features/session`: the screen controller, the lobby module, the UI / audio mounting and logout. The feature modules
// of this task as handler modules of `app/features.ts` (`FEATURE_MODULES`, registered at boot step B5, before the
// WebSocket opens, in this order).

import type { HandlerModule } from '../../app/features';
import { toHandlerModule } from '../common/module';
import { loadingModule } from '../loading';
import { lobbyModule } from './lobbyModule';
import { sessionModule } from './sessionModule';

export { createGameAudio } from './audioMount';
export { createGameUi, wireUi } from './uiMount';
export { UiLayer, TITLE_SCREENS } from './UiLayer';
export { logoutOf, sessionModule } from './sessionModule';
export { lobbyModule, lobbyPartsOf } from './lobbyModule';
export type { LobbyParts } from './lobbyModule';
export { createLogoutController } from './logout';
export type { LogoutController, LogoutKind, LogoutStart } from './logout';

/** The feature modules built by the `screens` task: lobby (login, select, create, delete), loading, session. */
export const SCREEN_FEATURE_MODULES: readonly HandlerModule[] = [
  lobbyModule,
  loadingModule,
  sessionModule,
].map(toHandlerModule);
