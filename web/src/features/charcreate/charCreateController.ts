// The character creation controller (Plan/45 §5, WP-CFEAT-04; 12 §9, ref/02 §7.2): the `CharCreateView` /
// `CharCreateActions` behind `WndCreateChar`.
//
// State: sex (initial female), hair mesh 0..9 and head mesh 0..10 (random initial values, arrows with wrap-around), skin
// set fixed 0. The preview subject: the default hair colour of the shared rule (`lobbyRules.defaultHairColor`) and the
// Vagrant starter set of the `tables.job_items` row by sex.
//
// OK (`CWndCreateChar::OnChildNotify(WIDC_OK)`): pre-checks in the legacy order, the first failure opens a message box
// and keeps OK enabled: 1. the name through `lobbyRules.validateCharacterName` (12 §9.3, TIDs 2030/3341/2011/2012/2019;
// skipped when the wasm exports are absent), 2. the PIN: empty `...ERROR01`, format `...02`/`...03`
// (`lobbyRules.validateBankPin`), confirmation empty `...04`, mismatch `...05`. Then `CreatePlayer` is sent and the
// buttons stay disabled until `CharList` (close, back to select, the new slot selected) or `LobbyError`.

import { create } from '@bufbuild/protobuf';
import { beginLobbyRequest } from '../../game/session/actions';
import { APP_CREATE_CHAR } from '../../gen/defs/resdata';
import {
  TID_2ND_PASSWORD_INPUT_ERROR01,
  TID_2ND_PASSWORD_INPUT_ERROR02,
  TID_2ND_PASSWORD_INPUT_ERROR04,
  TID_2ND_PASSWORD_INPUT_ERROR05,
} from '../../gen/defs/text';
import { CreatePlayerSchema } from '../../gen/proto/flyff/v1/lobby_pb';
import type { MoverVisualState } from '../../game/entities';
import { MI_FEMALE, MI_MALE } from '../../gen/defs/obj';
import type { CharCreateActions, CharCreateView, ViewSource } from '../../ui/windows/login/ports';
import { WndCreateChar } from '../../ui/windows/login/WndCreateChar';
import { previewPortOf } from '../charselect/previews';
import { playerVisual, starterEquipment } from '../charselect/subject';
import type { FeatureContext } from '../common/module';
import { nameRulesBytes } from '../common/lobbyRules';
import { formatTid } from '../common/text';
import { derivedView, localState } from '../common/view';

const noEquipment = (): ReturnType<typeof starterEquipment> => new Map();

/** `MAX_BASE_HAIR`, `MAX_DEFAULT_HEAD` (`defineNeuz.h:1134-1137`): hair meshes 0..9, head meshes 0..10. */
export const HAIR_MESHES = 10;
export const HEAD_MESHES = 11;
/** `SEX_FEMALE`: the initial sex of the window. */
export const INITIAL_SEX = 1;

export interface CharCreateController {
  readonly model: ViewSource<CharCreateView>;
  readonly actions: CharCreateActions;
  /** Opens the window with fresh random hair and head meshes (the legacy `OnInitialUpdate`). */
  show(): void;
  hide(): void;
  /** The current state (tests, the probe). */
  readonly state: () => { sex: number; hairMesh: number; headMesh: number; busy: boolean };
}

export interface CharCreateControllerDeps {
  /** Test seam: `xRandom(0, n)`. */
  readonly random?: (n: number) => number;
}

const wrap = (v: number, n: number): number => ((v % n) + n) % n;

export function createCharCreateController(
  fc: FeatureContext,
  deps: CharCreateControllerDeps = {},
): CharCreateController {
  const ctx = fc.app;
  const random = deps.random ?? ((n: number) => Math.floor(Math.random() * n));
  const local = localState({ sex: INITIAL_SEX, hairMesh: 0, headMesh: 0 });
  const session = ctx.stores.sessionStore;
  const partsOf = (id: number): number | undefined => ctx.data?.item(id)?.parts;

  let subject: { sig: string; value: MoverVisualState } | null = null;

  const subjectNow = (): MoverVisualState => {
    const { sex, hairMesh, headMesh } = local.get();
    const rules = fc.lobbyRules;
    const color = rules?.defaultHairColor(sex, hairMesh) ?? 0x00ffffff;
    const jobItems = ctx.bundles.get('tables')?.job_items;
    const sig = [
      sex,
      hairMesh,
      headMesh,
      color,
      jobItems ? 'j' : '-',
      ctx.bundles.has('items') ? 'i' : '-',
    ].join('|');
    if (subject?.sig === sig) return subject.value;
    const s: 0 | 1 = sex === 1 ? 1 : 0;
    const value = playerVisual({
      id: 0x7ffe_0001,
      sex: s,
      moverIndex: s === 1 ? MI_FEMALE : MI_MALE,
      hairMesh,
      hairColor: color,
      headMesh,
      skinSet: 0,
      equip: jobItems ? starterEquipment(jobItems, s, partsOf) : noEquipment(),
    });
    subject = { sig, value };
    return value;
  };

  const busy = (): boolean => {
    const s = session.getState().screen;
    return s.s === 'CharCreate' ? s.busy : true;
  };

  const model = derivedView<CharCreateView>(
    () => ({
      sex: local.get().sex,
      busy: busy(),
      subject: subjectNow(),
      maxNameLength: ctx.features.gameplay.max_name_letters,
    }),
    [local, session, ctx.bundles],
  );

  const msg = (tid: number, args: readonly string[] = []): void => {
    fc.ui?.msgbox.open(formatTid(fc.texts, tid, args));
  };

  const ok: CharCreateActions['ok'] = (rawName, pin, confirm) => {
    const screen = session.getState().screen;
    if (screen.s !== 'CharCreate' || screen.busy) return;
    const rules = fc.lobbyRules;
    const names = ctx.bundles.get('names');
    const gameplay = ctx.features.gameplay;
    // 1. the name (rules 1-6 of 12 §9.3 through the shared wasm rules; without them the server answers the same TIDs)
    let name = rawName.trim();
    if (rules && names) {
      const r = rules.validateCharacterName(
        rawName,
        nameRulesBytes(names),
        gameplay.min_name_letters,
        gameplay.max_name_letters,
      );
      if (r.tid !== undefined) {
        msg(r.tid);
        return;
      }
      name = r.name ?? name;
    }
    // 2. the bank PIN, in the order of the legacy client
    if (pin === '') {
      msg(TID_2ND_PASSWORD_INPUT_ERROR01);
      return;
    }
    if (rules) {
      const tid = rules.validateBankPin(pin);
      if (tid !== 0) {
        msg(tid, tid === TID_2ND_PASSWORD_INPUT_ERROR02 ? ['4'] : []);
        return;
      }
    }
    if (confirm === '') {
      msg(TID_2ND_PASSWORD_INPUT_ERROR04);
      return;
    }
    if (pin !== confirm) {
      msg(TID_2ND_PASSWORD_INPUT_ERROR05);
      return;
    }
    // 3. send; the single-flight rule (SM2) refuses a second request
    if (!beginLobbyRequest(ctx, 'create')) return;
    const { sex, hairMesh, headMesh } = local.get();
    // legacy-send: SendCreatePlayer
    fc.net.send({
      case: 'createPlayer',
      value: create(CreatePlayerSchema, {
        slot: screen.slot,
        name,
        skinSet: 0,
        hairMesh,
        sex,
        headMesh,
        bankPin: pin,
      }),
    });
    fc.net.flush();
  };

  const actions: CharCreateActions = {
    setSex: (sex) => {
      local.set({ sex: sex === 0 ? 0 : 1 });
    },
    stepHair: (d) => {
      local.set({ hairMesh: wrap(local.get().hairMesh + d, HAIR_MESHES) });
    },
    stepHead: (d) => {
      local.set({ headMesh: wrap(local.get().headMesh + d, HEAD_MESHES) });
    },
    ok,
    cancel: () => {
      ctx.machine.dispatch({ type: 'closeCreate' });
    },
  };

  return {
    model,
    actions,
    show() {
      const ui = fc.ui;
      if (!ui) return;
      // the window is already up (a busy toggle re-runs the screen reconcile): neither re-randomise nor reset the sex
      if (ui.wm.isOpen(APP_CREATE_CHAR)) return;
      local.set({
        sex: INITIAL_SEX,
        hairMesh: random(HAIR_MESHES),
        headMesh: random(HEAD_MESHES),
      });
      ui.wm.open(APP_CREATE_CHAR, () => new WndCreateChar(model, actions, previewPortOf(ctx)));
    },
    hide() {
      const ui = fc.ui;
      if (!ui) return;
      ui.wm.close(APP_CREATE_CHAR);
      ui.wm.flushDestroyed();
    },
    state: () => ({ ...local.get(), busy: busy() }),
  };
}
