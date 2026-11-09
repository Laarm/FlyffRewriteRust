// The character select controller (Plan/45 §4, WP-CFEAT-03; 12 §8.4, §10, §11, §18.2, §18.4): the `CharSelectView` /
// `CharSelectActions` behind `WndSelectChar`, and the delete dialog (`WndDeleteChar`, `DelPlayer`).
//
//  * selection: a blocked slot cannot be selected (a click or arrow shows `TID_DIAG_0073`); after every `CharList` the
//    selection follows the legacy `OnPlayerList` rule (`lobbyStore.applyCharList`);
//  * buttons: Create opens the create window on an empty slot, Delete the password dialog, Start / double click / Enter
//    sends `EnterWorld`, Back closes the socket, drops the token and shows the login window;
//  * every request goes through the machine's single-flight rule (SM2); `busy` disables the buttons until `CharList` or
//    `LobbyError`.

import { create } from '@bufbuild/protobuf';
import { createAuthActions, type AuthActions } from '../../app/authActions';
import { enterWorld, quit } from '../../game/session/actions';
import { beginLobbyRequest } from '../../game/session/actions';
import { APP_DELETE_CHAR, APP_SELECT_CHAR } from '../../gen/defs/resdata';
import {
  TID_APP_CHARACTER_JOB,
  TID_DIAG_0044,
  TID_DIAG_0073,
  TID_GAME_TOOL_EX_DEX,
  TID_GAME_TOOL_EX_INT,
  TID_GAME_TOOL_EX_LEVEL,
  TID_GAME_TOOL_EX_STA,
  TID_GAME_TOOL_EX_STR,
  TID_GAME_TOOLTIP_SEXFEMALE,
  TID_GAME_TOOLTIP_SEXMALE,
  TID_GAME_WND_SELECT_CHARACTER_EMPTY,
} from '../../gen/defs/text';
import { DelPlayerSchema } from '../../gen/proto/flyff/v1/lobby_pb';
import type { MoverVisualState } from '../../game/entities';
import { WndDeleteChar } from '../../ui/windows/login/WndDeleteChar';
import { WndSelectChar } from '../../ui/windows/login/WndSelectChar';
import type {
  CharDeleteActions,
  CharDeleteView,
  CharSelectActions,
  CharSelectView,
  CharSlotView,
  ViewSource,
} from '../../ui/windows/login/ports';
import { formatTid } from '../common/text';
import type { FeatureContext } from '../common/module';
import { derivedView } from '../common/view';
import type { LobbySlot, LobbyStore } from '../session/lobbyStore';
import { previewPortOf } from './previews';
import { slotSubject } from './subject';

/** `TID_DIAG_0044`'s legacy use: a delete key that is empty or 64 bytes or longer. */
const MAX_DELETE_KEY = 63;

export interface CharSelectController {
  readonly model: ViewSource<CharSelectView>;
  readonly deleteModel: ViewSource<CharDeleteView>;
  readonly actions: CharSelectActions;
  readonly deleteActions: CharDeleteActions;
  show(): void;
  hide(): void;
  /** Opens or closes the delete dialog to match `lobbyStore.deleteDialog` (called after every store change). */
  syncDelete(): void;
}

export function createCharSelectController(
  fc: FeatureContext,
  lobby: LobbyStore,
  auth: AuthActions = createAuthActions(fc.app),
): CharSelectController {
  const ctx = fc.app;
  const session = ctx.stores.sessionStore;
  const text = (tid: number): string => fc.texts.text(tid);
  const partsOf = (id: number): number | undefined => ctx.data?.item(id)?.parts;
  const subjects = new Map<number, { sig: string; subject: MoverVisualState }>();

  const subjectOf = (s: LobbySlot): MoverVisualState => {
    // the signature covers everything the visual reads, so an unchanged character keeps its subject object
    const sig = `${String(s.moverIndex)}|${String(s.sex)}|${String(s.hairMesh)}|${String(s.hairColor)}|${String(
      s.headMesh,
    )}|${String(s.skinSet)}|${s.equip.join(',')}|${ctx.bundles.has('items') ? 'i' : '-'}`;
    const known = subjects.get(s.idPlayer);
    if (known?.sig === sig) return known.subject;
    const subject = slotSubject(s, partsOf);
    subjects.set(s.idPlayer, { sig, subject });
    return subject;
  };

  const jobName = (job: number): string => ctx.data?.job(job)?.name ?? '';

  /** The legacy hover text (`PutToolTip_Character`): name, sex, level, STR/STA/DEX/INT, class. */
  const infoText = (s: CharSlotView): string => {
    const name = s.blocked
      ? `#b#cffff0000${s.name}\nConnet Cut#nc#nb`
      : `#b#cff0000ff${s.name}#nc#nb`;
    const sex = text(s.idSex === 1 ? TID_GAME_TOOLTIP_SEXFEMALE : TID_GAME_TOOLTIP_SEXMALE);
    return [
      name,
      sex,
      formatTid(fc.texts, TID_GAME_TOOL_EX_LEVEL, [s.level]),
      formatTid(fc.texts, TID_GAME_TOOL_EX_STR, [s.str]),
      formatTid(fc.texts, TID_GAME_TOOL_EX_STA, [s.sta]),
      formatTid(fc.texts, TID_GAME_TOOL_EX_DEX, [s.dex]),
      formatTid(fc.texts, TID_GAME_TOOL_EX_INT, [s.int]),
      `${text(TID_APP_CHARACTER_JOB)} : ${s.jobName}`,
    ].join('\n');
  };

  const model = derivedView<CharSelectView>(
    () => {
      const st = lobby.getState();
      const screen = session.getState().screen;
      const busy = screen.s === 'CharSelect' ? screen.busy : true;
      const slots = st.slots.map((s): CharSlotView | null =>
        s === null
          ? null
          : {
              idPlayer: s.idPlayer,
              name: s.name,
              blocked: s.blocked,
              level: s.level,
              jobName: jobName(s.job),
              str: s.str,
              sta: s.sta,
              dex: s.dex,
              int: s.int,
              idSex: s.sex,
              subject: subjectOf(s),
            },
      );
      return {
        slots,
        selected: st.selected,
        busy,
        emptyLabel: text(TID_GAME_WND_SELECT_CHARACTER_EMPTY),
        infoText,
      };
    },
    [lobby, session, ctx.bundles],
    (a, b) =>
      a.selected === b.selected &&
      a.busy === b.busy &&
      a.emptyLabel === b.emptyLabel &&
      a.slots.length === b.slots.length &&
      a.slots.every((s, i) => s === b.slots[i] || sameSlot(s, b.slots[i] ?? null)),
  );

  const isBusy = (): boolean => {
    const s = session.getState().screen;
    return s.s !== 'CharSelect' || s.busy;
  };

  const actions: CharSelectActions = {
    select(slot) {
      if (isBusy()) return;
      const s = lobby.getState().slots[slot] ?? null;
      if (s?.blocked === true) {
        fc.ui?.msgbox.open(text(TID_DIAG_0073)); // `CWndCharBlockBox`
        return;
      }
      if (lobby.getState().selected !== slot) lobby.setState({ selected: slot });
    },
    create(slot) {
      if (isBusy()) return;
      // an empty slot only; the machine refuses an occupied or out-of-range one
      ctx.machine.dispatch({ type: 'openCreate', slot });
    },
    remove(slot) {
      if (isBusy()) return;
      const s = lobby.getState().slots[slot] ?? null;
      if (s === null || s.blocked) return;
      lobby.setState((st) => ({
        deleteDialog: { slot, busy: false, clearNonce: st.deleteDialog.clearNonce },
      }));
    },
    start(slot) {
      const s = lobby.getState().slots[slot] ?? null;
      if (s === null || s.blocked) return;
      // `SendPreJoin` + `SendJoin`: the machine sends EnterWorld and shows the "Connecting" state (single-flight, SM2)
      enterWorld(ctx, s.slot, s.idPlayer);
    },
    back() {
      // Back: close the WebSocket with 1000, AWAIT the close, drop the token, show the login window (12 §18.2)
      void quit(ctx);
    },
  };

  // ---- the delete dialog ---------------------------------------------------------------------------------------------

  const deleteModel = derivedView<CharDeleteView>(() => {
    const d = lobby.getState().deleteDialog;
    return {
      busy: d.busy,
      clearNonce: d.clearNonce,
      // `auth.delete_requires_password = true`: the account password; else the layout's legacy "key" caption
      label:
        session.getState().authRules?.delete_requires_password === false ? '' : 'Account password',
    };
  }, [lobby, session]);

  const deleteActions: CharDeleteActions = {
    ok(key) {
      const d = lobby.getState().deleteDialog;
      if (d.slot === null || d.busy) return;
      const s = lobby.getState().slots[d.slot] ?? null;
      if (s === null) return;
      if (key.length === 0 || key.length > MAX_DELETE_KEY) {
        // the legacy client refused an empty or too long key and cleared the field
        fc.ui?.msgbox.open(text(TID_DIAG_0044));
        lobby.setState((st) => ({
          deleteDialog: { ...st.deleteDialog, clearNonce: st.deleteDialog.clearNonce + 1 },
        }));
        return;
      }
      if (!beginLobbyRequest(ctx, 'delete')) return; // a request is in flight (SM2)
      lobby.setState((st) => ({ deleteDialog: { ...st.deleteDialog, busy: true } }));
      // legacy-send: SendDeletePlayer
      fc.net.send({
        case: 'delPlayer',
        value: create(DelPlayerSchema, { idPlayer: s.idPlayer, pwhash: auth.pwHash(key) }),
      });
      fc.net.flush();
    },
    cancel() {
      lobby.setState((st) => ({
        deleteDialog: { slot: null, busy: false, clearNonce: st.deleteDialog.clearNonce },
      }));
    },
  };

  const closeApp = (app: number): void => {
    const ui = fc.ui;
    if (!ui) return;
    ui.wm.close(app);
    ui.wm.flushDestroyed();
  };

  return {
    model,
    deleteModel,
    actions,
    deleteActions,
    show() {
      const ui = fc.ui;
      if (!ui) return;
      ui.wm.open(APP_SELECT_CHAR, () => new WndSelectChar(model, actions, previewPortOf(ctx)));
    },
    hide() {
      closeApp(APP_DELETE_CHAR);
      closeApp(APP_SELECT_CHAR);
    },
    syncDelete() {
      const ui = fc.ui;
      if (!ui) return;
      const d = lobby.getState().deleteDialog;
      const open = ui.wm.isOpen(APP_DELETE_CHAR);
      if (d.slot !== null && !open) {
        ui.wm.open(APP_DELETE_CHAR, () => new WndDeleteChar(deleteModel, deleteActions));
      } else if (d.slot === null && open) {
        closeApp(APP_DELETE_CHAR);
      }
    },
  };
}

function sameSlot(a: CharSlotView | null, b: CharSlotView | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.idPlayer === b.idPlayer &&
    a.name === b.name &&
    a.blocked === b.blocked &&
    a.level === b.level &&
    a.jobName === b.jobName &&
    a.str === b.str &&
    a.sta === b.sta &&
    a.dex === b.dex &&
    a.int === b.int &&
    a.idSex === b.idSex &&
    a.subject === b.subject
  );
}
