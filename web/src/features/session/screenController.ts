// The screen controller (Plan/45 §1.6, rule SM1): doc 40's state machine decides the `ScreenState`; this module opens and
// closes the title-state windows in reaction to it (it never changes the state itself):
//
//   Login{notice}    the login window; a notice shows ONE `APP_MESSAGEBOX` (OK returns to the plain login body)
//   Connecting       the login window stays (if any) under the "Connecting. Please wait." box
//   CharSelect       `APP_SELECT_CHAR` (+ the delete dialog while `lobbyStore.deleteDialog.slot` is set)
//   CharCreate       `APP_CREATE_CHAR` instead of the select window (the legacy destroys one to show the other)
//   Entering         the select window under the "Connecting" box
//   Loading/InGame   every lobby window closed, the preview renderer released (the world renderer takes over)

import { APP_LOGIN } from '../../gen/defs/resdata';
import type { ScreenState, UiNotice } from '../../stores/screen';
import type { CharCreateController } from '../charcreate/charCreateController';
import type { CharSelectController } from '../charselect/charSelectController';
import { disposePreviews } from '../charselect/previews';
import type { FeatureContext } from '../common/module';
import type { LoginController } from '../login/loginController';
import type { LobbyStore } from './lobbyStore';

export interface ScreenControllerParts {
  readonly login: LoginController;
  readonly select: CharSelectController;
  readonly create: CharCreateController;
  readonly lobby: LobbyStore;
}

export interface ScreenController {
  /** Applies the current screen once (boot registers after the machine may already have left `Boot`). */
  reconcile(screen: ScreenState): void;
  dispose(): void;
}

const sameNotice = (a: UiNotice | undefined, b: UiNotice | undefined): boolean => {
  if (a === b) return true;
  if (a === undefined || b === undefined) return false;
  return a.tid === b.tid && a.text === b.text;
};

export function createScreenController(
  fc: FeatureContext,
  parts: ScreenControllerParts,
): ScreenController {
  const { login, select, create, lobby } = parts;
  let shownNotice: UiNotice | undefined;
  let connectingUp = false;

  const closeLobbyWindows = (): void => {
    select.hide();
    create.hide();
    login.hide();
    if (connectingUp) {
      login.closeConnecting();
      connectingUp = false;
    }
  };

  const showConnecting = (): void => {
    if (connectingUp) return;
    connectingUp = true;
    login.showConnecting();
  };
  const closeConnecting = (): void => {
    if (!connectingUp) return;
    connectingUp = false;
    login.closeConnecting();
  };

  // the windows are built from the `resdata-layouts` bundle: without it (a server without the game data) there is nothing to
  // open and the DOM status line of the screen body is all the page shows; the bundle's arrival reconciles again
  const layoutsReady = (): boolean => fc.ui?.registry.byId(APP_LOGIN) !== undefined;

  const reconcile = (screen: ScreenState): void => {
    if (!fc.ui || !layoutsReady()) return;
    switch (screen.s) {
      case 'Login':
        select.hide();
        create.hide();
        closeConnecting();
        login.show();
        if (screen.notice === undefined) {
          shownNotice = undefined;
        } else if (!sameNotice(shownNotice, screen.notice)) {
          shownNotice = screen.notice;
          login.showNotice(screen.notice);
        }
        break;
      case 'Connecting':
        showConnecting();
        break;
      case 'CharSelect':
        closeConnecting();
        login.hide();
        create.hide();
        select.show();
        select.syncDelete();
        break;
      case 'CharCreate':
        closeConnecting();
        select.hide();
        create.show();
        break;
      case 'Entering':
        select.show();
        showConnecting();
        break;
      case 'Loading':
      case 'InGame':
        closeLobbyWindows();
        lobby.setState((s) =>
          s.deleteDialog.slot === null ? s : { deleteDialog: { ...s.deleteDialog, slot: null } },
        );
        disposePreviews(fc.app);
        break;
      case 'Boot':
      case 'Leaving':
      case 'Reconnecting':
      case 'Reloading':
      case 'Fatal':
        break;
    }
  };

  const offMachine = fc.app.machine.subscribe((screen) => {
    reconcile(screen);
  });
  const offBundles = fc.app.bundles.subscribe((name) => {
    if (name === 'resdata-layouts')
      queueMicrotask(() => {
        reconcile(fc.app.machine.screen);
      });
  });
  // the delete dialog follows the store (Delete / OK / Cancel / CharList / LobbyError change it)
  const offLobby = lobby.subscribe(() => {
    if (fc.app.machine.screen.s === 'CharSelect') select.syncDelete();
  });

  return {
    reconcile,
    dispose() {
      offMachine();
      offBundles();
      offLobby();
    },
  };
}
