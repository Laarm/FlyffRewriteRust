// The `npc` feature, dialog part (Plan/45 §11.3; WP-CFEAT-18): the conversation window of an NPC. `DialogOps` messages are
// applied to the dialog store (`state.ts`), the window of 42 (`ui.npc.openDialog`) reads it through a view and answers with
// clicks, which become `ScriptDlg` requests (the server accepts only what it offered). The window closes when the NPC leaves
// the area, on `exit`, on Esc or when a transactional close runs (death, world change, logout).
//
// Other features reach it with `dialogOf(ctx)`: `open(npcObjId)` is the `MMI_DIALOG` action of the NPC menu.

import type { ClientContext } from '../../../app/context';
import { peekRulesRuntime } from '../../../game/rules';
import { APP_DIALOG_EX, APP_INVENTORY } from '../../../gen/defs/resdata';
import type { SpawnNpc } from '../../../gen/proto/flyff/v1/world_pb';
import { EntityKind } from '../../../game/entities';
import type { DialogActions, DialogClick, DialogView } from '../../../ui/windows/npc/ports';
import { addProbeMember } from '../../common/probe';
import { derivedView, type ViewSource } from '../../common/view';
import { type FeatureContext, fullUi } from '../../common/module';
import type { MenuFeature } from '../menuModule';
import { MMI_DIALOG } from '../../../gen/defs/neuz';
import { sendScriptDlg } from './senders';
import {
  acceptsOps,
  applyDialogOps,
  clickAnswer,
  clickKey,
  clickLink,
  clickQuest,
  defineDialogStore,
  type ClickResult,
  type DialogEffect,
  type DialogEnv,
  type DialogState,
  type DialogStore,
  initialDialogState,
  mayClick,
  nextPage,
} from './state';

/** What the quest data says about a quest id (the quest files are empty in this distribution, Plan/20 §12). */
export interface QuestInfo {
  readonly title: string;
  readonly levelMin: number;
  readonly levelMax: number;
}

/** The public face of the dialog feature of a context. */
export interface DialogFeature {
  readonly store: DialogStore;
  readonly view: ViewSource<DialogView>;
  readonly actions: DialogActions;
  /** `MMI_DIALOG`: opens the window for the NPC and sends the opening `ScriptDlg` (empty key = `#init`). */
  open(npcObjId: number): boolean;
  /** Closes the window (no message). */
  close(): void;
  isOpen(): boolean;
  state(): DialogState;
  /** The quest lookup of the quest feature (WP-CFEAT-42); the default knows no quest. */
  setQuestLookup(fn: (questId: number) => QuestInfo | null): void;
  /** `__IsEndQuestCondition`: which current quests can be completed now (the yellow icon). */
  setCompletable(fn: (questId: number) => boolean): void;
}

const features = new WeakMap<ClientContext, DialogFeature>();

/** The dialog feature of a context; `undefined` before the module registered. */
export function dialogOf(ctx: ClientContext): DialogFeature | undefined {
  return features.get(ctx);
}

export function registerDialog(fc: FeatureContext, menu: MenuFeature): DialogFeature {
  const ctx = fc.app;
  const store = defineDialogStore(ctx.stores.registry);
  let questLookup: (id: number) => QuestInfo | null = () => null;
  let completable: (id: number) => boolean = () => false;
  const stats = { applied: 0, dropped: 0, sent: 0, throttled: 0 };

  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const env: DialogEnv = {
    text,
    quest: (id) => questLookup(id),
    diag: {
      stray: (word, t) => {
        ctx.log.warn('npc', `dialog text: "${t}" between ] and [ after the word "${word}"`);
      },
    },
  };

  // ---- the view ----------------------------------------------------------------------------------------------------

  let cached: { state: DialogState; dataVersion: number; view: DialogView } | null = null;
  const build = (): DialogView => {
    const st = store.getState();
    const dv = ctx.data?.version ?? -1;
    if (cached?.state === st && cached.dataVersion === dv) return cached.view;
    const row = st.npcKey === '' ? undefined : ctx.data?.npc(st.npcKey);
    const done = new Set<number>();
    for (const q of st.currentQuests) if (completable(q.questId)) done.add(q.questId);
    const view: DialogView = {
      version: st.version,
      npcId: st.npcId,
      portrait: row && row.portrait !== '' ? row.portrait : null,
      title: st.title,
      say: st.say,
      selectedKey: st.selectedKey,
      keys: st.keys,
      answers: st.answers,
      pageCount: st.pages.length,
      page: st.page,
      shown: st.shown,
      newQuests: st.newQuests,
      currentQuests: st.currentQuests,
      completable: done,
    };
    cached = { state: st, dataVersion: dv, view };
    return view;
  };
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  const view = derivedView(build, [store, dataSource], (a, b) => a === b);

  // ---- close ---------------------------------------------------------------------------------------------------------

  const close = (): void => {
    fullUi(ctx)?.wm.get(APP_DIALOG_EX)?.destroy();
    // the window's `onDestroy` resets the store; without a window (a test, a lost UI) the state goes now
    if (!fullUi(ctx)?.wm.get(APP_DIALOG_EX)) store.setState(initialDialogState());
  };

  // ---- the actions of the window -----------------------------------------------------------------------------------

  const send = (r: ClickResult | null, npcId: number): boolean => {
    if (!r) return false;
    store.setState(r.state);
    if (r.request) {
      stats.sent += 1;
      sendScriptDlg(ctx, npcId, r.request.key, r.request.param, r.request.questId);
    }
    return true;
  };
  const actions: DialogActions = {
    click(c: DialogClick): boolean {
      const st = store.getState();
      if (!st.open) return false;
      const now = ctx.now();
      if (!mayClick(st, now)) {
        stats.throttled += 1;
        return false;
      }
      switch (c.kind) {
        case 'key':
          return send(clickKey(st, c.index, now, env), st.npcId);
        case 'answer':
          return send(clickAnswer(st, c.index, now), st.npcId);
        case 'link':
          return send(clickLink(st, c.key, now), st.npcId);
        case 'quest':
          return send(clickQuest(st, c.list, c.index, now), st.npcId);
      }
    },
    next() {
      store.setState(nextPage(store.getState()));
    },
    closed() {
      store.setState(initialDialogState());
    },
  };

  // ---- opening --------------------------------------------------------------------------------------------------------

  const open = (npcObjId: number): boolean => {
    const ui = fullUi(ctx);
    const rec = ctx.entities.get(npcObjId);
    if (!ui || rec?.kind !== EntityKind.Npc) return false;
    const key = (rec.spawn as SpawnNpc).npc?.characterKey ?? '';
    // a window that is still open (or waits for its deferred destruction) is replaced (`SAFE_DELETE(m_pWndDialog)`)
    const old = ui.wm.get(APP_DIALOG_EX);
    if (old) {
      old.destroy();
      ui.wm.flushDestroyed();
    }
    const s0 = store.getState();
    store.setState({
      ...initialDialogState(),
      open: true,
      npcId: npcObjId,
      npcKey: key,
      version: s0.version + 1,
    });
    // legacy order: the request leaves first, then the window is created
    sendScriptDlg(ctx, npcObjId, '', 0, 0);
    stats.sent += 1;
    // `OnInitialUpdate`: the inventory window closes
    ui.wm.get(APP_INVENTORY)?.destroy();
    const w = ui.npc.openDialog({ view, actions });
    if (!w) {
      store.setState(initialDialogState());
      return false;
    }
    ctx.audio.cues?.dialogOpened(key);
    return true;
  };

  // ---- the effects of operations outside the dialog -----------------------------------------------------------

  const applyEffects = (effects: readonly DialogEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'exit':
          close();
          break;
        case 'initStat': {
          const all = e.stat === 'all';
          ctx.stores.selfStore.setState((s) => ({
            ...s,
            base: {
              str: all || e.stat === 'str' ? 15 : s.base.str,
              sta: all || e.stat === 'sta' ? 15 : s.base.sta,
              dex: all || e.stat === 'dex' ? 15 : s.base.dex,
              int: all || e.stat === 'int' ? 15 : s.base.int,
            },
            statPoints: e.value,
          }));
          peekRulesRuntime(ctx)?.markSelfStatsDirty();
          break;
        }
        case 'renamePlayer':
        case 'navigator':
          // the free rename window (M6) and the minimap navigator (45 §7.6) are other features: nothing to do yet
          ctx.log.debug('npc', `dialog effect ${e.kind} is not built yet`);
          break;
      }
    }
  };

  fc.net.on('dialogOps', (m) => {
    const st = store.getState();
    if (!acceptsOps(st, m.npcObjId, m.sessionSeq)) {
      stats.dropped += 1;
      return;
    }
    const r = applyDialogOps(st, m.ops, m.sessionSeq, env);
    stats.applied += 1;
    store.setState(r.state);
    applyEffects(r.effects);
  });

  // the NPC leaves the area: the window goes with it (`CWndDialog::Process`)
  ctx.entities.onRemove((rec) => {
    const st = store.getState();
    if (st.open && rec.objId === st.npcId) close();
  });
  fc.transactional.register({ id: 'npc.dialog', reasons: 'all', close });

  // `MMI_DIALOG`: ignored within three seconds of an item drop and while a personal shop window is open
  menu.registerAction(MMI_DIALOG, ({ targetId }) => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const dropAt = menu.lastDropAt();
    if (dropAt !== null && ui.host.now() - dropAt < 3000) return;
    if (ui.wm.isVendorWindowOpen()) return;
    open(targetId);
  });

  const feature: DialogFeature = {
    store,
    view,
    actions,
    open,
    close,
    isOpen: () => store.getState().open,
    state: () => store.getState(),
    setQuestLookup(fn) {
      questLookup = fn;
    },
    setCompletable(fn) {
      completable = fn;
    },
  };
  features.set(ctx, feature);

  addProbeMember(ctx, 'dialog', () => {
    const s = store.getState();
    return {
      open: s.open,
      npcId: s.npcId,
      npcKey: s.npcKey,
      seq: s.seq,
      title: s.title,
      say: s.say,
      keys: s.keys.map((k) => ({ word: k.word, key: k.key })),
      answers: s.answers.map((k) => ({ word: k.word, key: k.key })),
      pages: s.pages.length,
      page: s.page,
      text: s.shown ? s.shown.runs.map((r) => r.text).join('') : null,
      links: s.shown ? [...s.shown.links.entries()] : [],
      newQuests: s.newQuests.map((q) => q.word),
      currentQuests: s.currentQuests.map((q) => q.word),
      stats: { ...stats },
    };
  });
  return feature;
}
