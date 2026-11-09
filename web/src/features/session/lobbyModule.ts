// The `lobby` feature module (Plan/45 §3-§5, WP-CFEAT-02/03/04): registers the `lobbyStore`, the `CharList` / `LobbyError`
// handlers (phase Feature: they run after the Core handlers that fed the state machine), the controllers of the login,
// character select, create and delete windows, and the screen controller that opens them in reaction to the state.

import type { ClientContext } from '../../app/context';
import { lobbyErrorText } from '../../auth/errors';
import { TID_DIAG_0073 } from '../../gen/defs/text';
import {
  createCharCreateController,
  type CharCreateController,
} from '../charcreate/charCreateController';
import {
  createCharSelectController,
  type CharSelectController,
} from '../charselect/charSelectController';
import type { FeatureContext, FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import { createLoginController, type LoginController } from '../login/loginController';
import { applyCharList, defineLobbyStore, type LobbyStore } from './lobbyStore';
import { installLobbyE2eProbe } from './e2eProbe';
import { createScreenController, type ScreenController } from './screenController';
import { previewServiceOf } from '../charselect/previews';

/** Codes after which the delete dialog clears its field and enables its buttons again (12 §16.2). */
const DELETE_REFUSED = new Set([1319, 112]);

export function registerLobbyHandlers(fc: FeatureContext, lobby: LobbyStore): void {
  // CharList: store the slots; the machine moved to CharSelect (also out of CharCreate), the delete window closes
  fc.net.on('charList', (list) => {
    lobby.setState((prev) => ({
      ...applyCharList(prev, list),
      lastError: null,
      deleteDialog: { slot: null, busy: false, clearNonce: prev.deleteDialog.clearNonce },
      listVersion: prev.listVersion + 1,
    }));
  });
  // LobbyError: the text of 12 §16.2 in a message box; a refused delete clears its field
  fc.net.on('lobbyError', (err) => {
    const prev = lobby.getState();
    const text = lobbyErrorText(err.code, err.tid, (tid) => fc.texts.text(tid));
    const refusedDelete = prev.deleteDialog.slot !== null && DELETE_REFUSED.has(err.code);
    lobby.setState({
      lastError: { code: err.code, tid: err.tid, seq: (prev.lastError?.seq ?? 0) + 1 },
      deleteDialog:
        prev.deleteDialog.slot === null
          ? prev.deleteDialog
          : {
              slot: prev.deleteDialog.slot,
              busy: false,
              clearNonce: prev.deleteDialog.clearNonce + (refusedDelete ? 1 : 0),
            },
    });
    fc.ui?.msgbox.open(err.code === 2001 ? fc.texts.text(TID_DIAG_0073) : text);
  });
}

/** What the lobby module built for a context (the e2e drivers and the tests reach the controllers through it). */
export interface LobbyParts {
  readonly lobby: LobbyStore;
  readonly login: LoginController;
  readonly select: CharSelectController;
  readonly create: CharCreateController;
  readonly screens: ScreenController;
}

const parts = new WeakMap<ClientContext, LobbyParts>();

/** The controllers of the lobby screens of a context (`undefined` before the module registered). */
export function lobbyPartsOf(ctx: ClientContext): LobbyParts | undefined {
  return parts.get(ctx);
}

export const lobbyModule: FeatureModule = {
  id: 'lobby',
  milestone: 1,
  register(fc) {
    const lobby = defineLobbyStore(fc.app.stores.registry);
    registerLobbyHandlers(fc, lobby);
    const login = createLoginController(fc);
    const select = createCharSelectController(fc, lobby);
    const create = createCharCreateController(fc);
    const screens = createScreenController(fc, { login, select, create, lobby });
    parts.set(fc.app, { lobby, login, select, create, screens });
    addProbeMember(fc.app, 'lobby', () => {
      const st = lobby.getState();
      return {
        screen: fc.app.machine.screen.s,
        selected: st.selected,
        slots: st.slots.map((s) =>
          s === null
            ? null
            : {
                slot: s.slot,
                idPlayer: s.idPlayer,
                name: s.name,
                blocked: s.blocked,
                level: s.level,
              },
        ),
        lastError: st.lastError,
        deleteOpen: st.deleteDialog.slot !== null,
      };
    });
    installLobbyE2eProbe(fc);
    addProbeMember(fc.app, 'previews', () => previewServiceOf(fc.app).probe());
    // the machine may already be past `Boot` when the module registers (tests): apply the current screen once
    screens.reconcile(fc.app.machine.screen);
  },
};
