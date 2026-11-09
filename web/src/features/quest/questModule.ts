// The `quest` feature (Plan/45 §28.1, §2.3, §7.1, §7.6; Plan/20 §9.12; PL-148; WP-CFEAT-42): the client side of the quest engine.
//
//   questStore            the quest book of the own character (`book.ts`): the lists of `SpawnSelf`, `SetQuest`, `RemoveQuestNtf`, `QuestChecked`, the guild quests, the timed text and the memory of the
//                         windows; applied in the normal handler phase (never `pre`: render-audio-m7's T38 binding reads the old book)
//   progress texts        the lines and captions a changed record prints (`progress.ts`), the legacy `RemoveQuestReq` after a timeout
//   the gate (PL-148)     `available() = the quests bundle is loaded and not empty`. The shipped data has no quests (PD-01): then NOTHING of the quest UI exists - no applet, no quick info, no detail, no
//                         NPC mark, no dialog hook, no join line, no quest-made destination. The bundle is ensured once the session is logged in; the decision is made once per session
//   windows               `ui.quest` over requests built here: the list applet, the detail, the quick info (at field entry), the Heaven Tower is ui-secret-tax's
//   marks                 `emoticonOf( objId )` for the minimap (`navigatorOf( ctx ).setQuestEmoticonSource`) and render-audio-m7; the literal V15 loop of `markers.ts`
//   destination           a click on a condition line, `QuestHelperNpcPos` (`features/navigator/destination.ts`)
//   seams                 `dialogOf( ctx ).setQuestLookup / setCompletable`, `questOf( ctx ).joinTitles()` for the join lines, `inputProvidersOf( ctx ).questDestination`
//
// Everything the client evaluates (marks, colours, condition lines) is display only: the server owns the quests.

import type { ClientContext } from '../../app/context';
import { inputProvidersOf } from '../../app/inputProviders';
import { EntityKind } from '../../game/entities';
import type { NpcRow, QuestClientDef, QuestsBundle } from '../../gen/bundles';
import { APP_DIALOG_EX, APP_QUEST_EX_LIST, APP_QUEST_QUICK_INFO } from '../../gen/defs/resdata';
import { QUEST_KIND_EVENT } from '../../gen/defs/quest';
import {
  TID_GAME_DESTINATION_BOX_TITLE,
  TID_GAME_ERROR_DONT_CHECK,
  TID_GAME_REMOVEQUEST,
} from '../../gen/defs/text';
import type { SpawnNpc } from '../../gen/proto/flyff/v1/world_pb';
import type { QuestListPorts } from '../../ui/windows/quest/ports';
import { QUEST_TAB_KINDS } from '../../ui/windows/quest/rows';
import { fullUi, type FeatureContext, type FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import { formatTid, SYSTEM_CHAT, sysChat } from '../common/text';
import { dialogOf } from '../npc/dialog/dialogModule';
import { goalAction, goalSearch } from '../navigator/destination';
import { AUTH_GAMEMASTER } from '../navigator/runtime';
import { navigatorOf } from '../navigator/module';
import { hudOf } from '../hud/hudModule';
import { definedTextActions, DISPLAY } from '../texts/format';
import { guildOf } from '../guild/api';
import { inventoryOf } from '../inventory';
import { partyOf } from '../party/partyModule';
import { petsOf } from '../pets/petsModule';
import { relationOf } from '../relation/relationModule';
import { skillTreeOf } from '../skills/tree/module';
import { setQuestOf, type QuestFeature } from './api';
import {
  applyChecked,
  applyGuildQuest,
  applyGuildQuestRemoved,
  applyRemove,
  applySetQuest,
  applyTextTime,
  bookFromSelf,
  EMPTY_QUEST_ID,
  findRecord,
  forgetQuest,
  quickOpen,
  recordOf,
  rememberQuest,
  setListMemory,
  setQuickOpen,
  type QuestBook,
} from './book';
import {
  completedLine,
  conditionLines,
  descriptionText,
  rewardText,
  type CondEnv,
  type CondLine,
  DETAIL_COLORS,
  type CondColors,
} from './conditions';
import { markerIndex } from './markers';
import { canEnd, type QuestSubject } from './predicates';
import { progressEffects, type ProgressEffect } from './progress';
import { questSenders } from './senders';
import { defineQuestStore } from './store';
import { clientSubject } from './subject';
import { buildQuestTree, rootHeadQuest } from './tree';
import type {
  QuestDetailRequest,
  QuestDetailView,
  QuestQuickInfoRequest,
  QuickQuestView,
} from '../../ui/windows/quest/ports';

export const questModule: FeatureModule = {
  id: 'quest',
  milestone: 7,
  register(fc) {
    registerQuest(fc);
  },
};

/** The ids the original never gives a join line (`DPClient.cpp:4393`: quest 255 is KEEP-QUIRK). */
const NO_JOIN_LINE = new Set<number>([0xffff, 0xff]);

/** The colours of the quick info's lines are recoloured every frame by the window; the lines carry the plain colours. */
const QUICK_COLORS: CondColors = { start: 0xffffffff, end: 0xffffffff, select: 0xffff0000 };

export function registerQuest(fc: FeatureContext): QuestFeature {
  const ctx: ClientContext = fc.app;
  const { selfStore } = ctx.stores;
  const store = defineQuestStore(ctx.stores.registry);
  const senders = questSenders((msg) => ctx.net.sender.send(msg));
  const book = (): QuestBook => store.getState();

  // ---- the data ----------------------------------------------------------------------------------------------------------------

  let defsFor: QuestsBundle | undefined;
  let defMap: ReadonlyMap<number, QuestClientDef> = new Map();
  // the data index of a test rig can be a partial fake: ask only for what exists
  const bundle = (): QuestsBundle | undefined => {
    const d = ctx.data;
    return d && typeof d.bundle === 'function' ? d.bundle('quests') : undefined;
  };
  const defs = (): ReadonlyMap<number, QuestClientDef> => {
    const b = bundle();
    if (b !== defsFor) {
      defsFor = b;
      defMap = new Map((b?.quests ?? []).map((q) => [q.id, q]));
    }
    return defMap;
  };
  const def = (id: number): QuestClientDef | undefined => defs().get(id);
  let npcsFor: unknown;
  let npcMap: ReadonlyMap<string, NpcRow> = new Map();
  /** `prj.GetCharacter( key )`: NPC rows by lower-case key. */
  const npcOf = (key: string): NpcRow | undefined => {
    const d = ctx.data;
    const b = d && typeof d.bundle === 'function' ? d.bundle('npcs') : undefined;
    if (b !== npcsFor) {
      npcsFor = b;
      npcMap = new Map((b?.npcs ?? []).map((n) => [n.key.toLowerCase(), n]));
    }
    return npcMap.get(key.toLowerCase());
  };

  // ---- the gate (PL-148) ---------------------------------------------------------------------------------------------------------

  let decided = false;
  let available = false;
  let requested = false;

  // ---- facts and change notification -----------------------------------------------------------------------------------------

  let tutorial = 0;
  const subject: QuestSubject = clientSubject(ctx, { book, tutorialState: () => tutorial });
  const fixes = (): boolean => ctx.features.fidelity.quest_condition_fixes;
  const isGm = (): boolean => selfStore.getState().authority >= AUTH_GAMEMASTER;

  /** Listeners of the book alone (the list) and of the book plus every fact the lines read (the detail, the quick info, the marks). */
  const bookListeners = new Set<() => void>();
  const anyListeners = new Set<() => void>();
  let version = 0;
  const fire = (set: Set<() => void>): void => {
    for (const fn of [...set]) fn();
  };
  /** The book changed. */
  const bumpBook = (): void => {
    version += 1;
    fire(bookListeners);
    fire(anyListeners);
  };
  /** A fact the lines and marks read changed (coalesced: inventory, party, self ...). */
  let scheduled = false;
  const bumpFacts = (): void => {
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      version += 1;
      fire(anyListeners);
    });
  };
  const onBook = (fn: () => void): (() => void) => {
    bookListeners.add(fn);
    return () => {
      bookListeners.delete(fn);
    };
  };
  const onAny = (fn: () => void): (() => void) => {
    anyListeners.add(fn);
    return () => {
      anyListeners.delete(fn);
    };
  };
  store.subscribe((s, prev) => {
    if (s !== prev) bumpBook();
  });

  // ---- text environments ----------------------------------------------------------------------------------------------------

  const condEnv = (): CondEnv => ({
    fmt: (tid, args) => formatTid(fc.texts, tid, args),
    sex: () => subject.sex(),
    job: () => subject.job(),
    itemNum: (i) => subject.itemNum(i),
    isDisguised: () => subject.isDisguised(),
    skillLevel: (s) => subject.skillLevel(s),
    checkSkill: (s) => subject.checkSkill(s),
    inParty: () => selfStore.getState().partyId !== 0,
    partySize: () => subject.party()?.size ?? 0,
    moverName: (id) => ctx.data?.mover(id)?.name ?? '',
    itemName: (id) => ctx.data?.item(id)?.name ?? '',
    skillName: (id) => ctx.data?.skill(id)?.name ?? '',
    npcName: (key) => npcOf(key)?.name,
    patrolName: (id) => {
      const v = bundle()?.patrol_destinations[String(id)];
      return typeof v === 'string' ? v : '';
    },
  });
  const nameOf = {
    fmt: (tid: number, args: readonly (string | number)[]) => formatTid(fc.texts, tid, args),
    moverName: (id: number) => ctx.data?.mover(id)?.name ?? '',
    npcName: (key: string) => npcOf(key)?.name ?? '',
  };

  // ---- the lines of a quest -------------------------------------------------------------------------------------------------------

  /** The condition lines of an active record (the end-character line carries `role: 'character'`). */
  const linesOf = (d: QuestClientDef, colors: CondColors): CondLine[] => {
    const rec = findRecord(book(), d.id);
    return rec ? conditionLines(d, rec, condEnv(), colors) : [];
  };

  const detailView = (questId: number): QuestDetailView | null => {
    const d = def(questId);
    const b = book();
    const rec = findRecord(b, questId);
    const completed = b.completed.includes(questId);
    if (!d || (!rec && !completed)) return null;
    const env = condEnv();
    return {
      description: descriptionText(d, rec ?? null, completed && !rec),
      lines: completed && !rec ? [completedLine(env)] : linesOf(d, DETAIL_COLORS),
      reward: rewardText(d, env),
    };
  };

  // ---- the destination --------------------------------------------------------------------------------------------------------------

  const destination = (): ReturnType<typeof navigatorOf> => navigatorOf(ctx);

  /** A click on a condition line: `SetQuestDestinationInformation` + `ProcessQuestDestinationWorldMap` (+ the NPC position request for the end character). */
  const goalClick = (questId: number, line: { data: number; role?: 'character' }): void => {
    const d = def(questId);
    const nav = destination();
    if (!d || !nav) return;
    nav.destination.clear();
    const r = goalSearch(d, line.data);
    if (r.point) nav.destination.set(r.point.x, r.point.z, false);
    const action = goalAction(nav.destination.get(), r.textId);
    if (action.kind === 'worldMap') {
      nav.worldMap.openDestination();
    } else if (action.kind === 'box') {
      const text = bundle()?.quest_destinations[String(action.textId)];
      fullUi(ctx)?.quest.host.boxWithTitle(
        fc.texts.text(TID_GAME_DESTINATION_BOX_TITLE),
        typeof text === 'string' ? text : '',
      );
    }
    // V13 asked the helper for the NPC on every click of a quest with an end character (not an event quest); V15 compiled it out, the Plan asks for it (45 §28.1)
    if (line.role === 'character' && rootHeadQuest(questId, def) !== QUEST_KIND_EVENT) {
      const npc = npcOf(d.end.character.npc);
      if (npc) senders.reqNpcPos(npc.key);
    }
  };

  const detailRequest = (questId: number): QuestDetailRequest => ({
    questId,
    subscribe: onAny,
    view: () => detailView(questId),
    clickLine: (line) => {
      goalClick(questId, line);
    },
  });

  /** The detail window toggles (`WndQuestQuickInfo`: the same quest closes it, another replaces it). */
  const toggleDetail = (questId: number): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.quest.detail()?.questId === questId) ui.quest.closeDetail();
    else {
      ui.quest.closeDetail();
      ui.quest.openDetail(detailRequest(questId));
    }
  };

  const quickRequest: QuestQuickInfoRequest = {
    subscribe: onAny,
    view: (): readonly QuickQuestView[] => {
      const b = book();
      const out: QuickQuestView[] = [];
      for (const id of b.checked) {
        const d = def(id);
        if (!d) continue;
        out.push({
          id,
          title: d.title,
          open: quickOpen(b.tree, id),
          lines: linesOf(d, QUICK_COLORS),
        });
      }
      return out;
    },
    checkedCount: () => book().checked.length,
    setOpen: (id, open) => {
      store.setState(setQuickOpen(book(), id, open));
    },
    clickTitle: (id) => {
      toggleDetail(id);
      // `SetQuestDestinationInformation( id, 0 )`: the destination is cleared
      destination()?.destination.clear();
    },
    clickLine: (id, line) => {
      goalClick(id, line);
    },
  };

  // ---- the list ------------------------------------------------------------------------------------------------------------------------

  const listPorts: QuestListPorts = {
    subscribe: onBook,
    tabTitle: (tab) => {
      const kind = QUEST_TAB_KINDS[tab];
      return kind === undefined ? '' : (def(kind)?.title ?? '');
    },
    tree: (group) => buildQuestTree(book(), group as 0 | 1 | 2, { def, isGm: isGm() }).tabs,
    checked: () => book().checked,
    memory: () => ({ tab: book().tree.tab, group: book().tree.group }),
    setMemory: (tab, group) => {
      store.setState(setListMemory(book(), tab, group as 0 | 1 | 2));
    },
    isGm,
    dialogOpen: () => fullUi(ctx)?.wm.isOpen(APP_DIALOG_EX) === true,
    quest: (id) => {
      const rec = findRecord(book(), id);
      const completed = book().completed.includes(id);
      if (!rec && !completed) return null;
      return {
        active: rec !== undefined,
        completed,
        state: rec?.state ?? 14,
        noRemove: def(id)?.no_remove === true,
      };
    },
    tick: (id, on) => {
      // the quick-tree entry (`InsertTreeInformation( id, TRUE )` / `DeleteTreeInformation`) and the send
      store.setState(on ? rememberQuest(book(), id) : forgetQuest(book(), id));
      senders.check(id, on);
    },
    tickRefused: () => {
      sysChat(fc.texts, fc.chat, TID_GAME_ERROR_DONT_CHECK);
    },
    remove: (id) => {
      // `CWndRemoveQuest`: only a quest that is still active
      if (findRecord(book(), id)) senders.remove(id);
    },
    detail: detailRequest,
  };

  // ---- the effects of a message -----------------------------------------------------------------------------------------------

  const run = (effects: readonly ProgressEffect[]): void => {
    const ui = fullUi(ctx);
    for (const e of effects) {
      switch (e.type) {
        case 'put':
          ui?.chat.putString(e.text, null, 0xffffffff, SYSTEM_CHAT);
          break;
        case 'caption':
          // `g_CapTime.SetTime( min )`: approximated by a small HUD caption of the same text (`WndWorld.cpp:354-430`)
          hudOf(ctx).caption(e.text, true, ctx.now());
          break;
        case 'removeReq':
          senders.remove(e.questId);
          break;
        case 'defined':
          for (const a of definedTextActions(fc.texts, e.tid, [], DISPLAY.MESSAGE)) {
            if (a.type === 'put') ui?.chat.putString(a.text, a.objId, a.color, a.style, a.speaker);
          }
          break;
      }
    }
  };

  const isSelf = (objId: number): boolean => objId === 0 || objId === selfStore.getState().objId;

  fc.net.on('spawnSelf', (m) => {
    tutorial = m.self?.tutorialState ?? 0;
    store.setState(bookFromSelf(book(), m.self));
  });

  fc.net.on('setQuest', (m, objId) => {
    if (!isSelf(objId) || !m.quest) return;
    const next = recordOf(m.quest);
    if (next.id === EMPTY_QUEST_ID) return;
    const prev = findRecord(book(), next.id);
    const d = def(next.id);
    // the texts compare with the record as it was (the original printed before it stored)
    run(progressEffects(prev, next, d, nameOf));
    const r = applySetQuest(book(), next, d ? d.repeat : undefined);
    if (r.outcome === 'ignored') return;
    store.setState(r.book);
    // the completion clears ANY destination (`m_vDestinationArrow = (-1, 0, -1)`)
    if (r.outcome === 'ended') destination()?.destination.clear();
  });

  fc.net.on('removeQuestNtf', (m, objId) => {
    if (!isSelf(objId)) return;
    // -1: the cancel text in its colour (FIX: nothing for an unknown definition; the original dereferenced null)
    if (m.mode === -1) {
      const d = def(m.questId);
      if (d) sysChat(fc.texts, fc.chat, TID_GAME_REMOVEQUEST, [d.title]);
    }
    store.setState(applyRemove(book(), m.mode, m.questId));
  });

  fc.net.on('questChecked', (m, objId) => {
    if (!isSelf(objId)) return;
    store.setState(applyChecked(book(), m.questIds));
  });

  fc.net.on('questTextTime', (m, objId) => {
    if (!isSelf(objId)) return;
    store.setState(applyTextTime(book(), m.flag, m.state, m.timeMs, ctx.now()));
  });

  fc.net.on('setGuildQuest', (m) => {
    store.setState(applyGuildQuest(book(), m.questId, m.state));
  });

  fc.net.on('removeGuildQuest', (m) => {
    store.setState(applyGuildQuestRemoved(book(), m.questId));
  });

  fc.net.on('questHelperNpcPos', (m) => {
    // `m_bSetQuestNPCDest = TRUE; m_vQuestNPCDest = vPos`
    if (m.pos) destination()?.destination.set(m.pos.x, m.pos.z, true);
  });

  // ---- the marks ----------------------------------------------------------------------------------------------------------------

  let markCache = new Map<string, number>();
  let markVersion = -1;
  const emoticonOf = (objId: number): number => {
    if (!available) return -1;
    const rec = ctx.entities.get(objId);
    if (!rec || rec.removed || rec.kind !== EntityKind.Npc) return -1;
    const key = ((rec.spawn as SpawnNpc).npc?.characterKey ?? '').toLowerCase();
    if (key === '') return -1;
    if (markVersion !== version) {
      markVersion = version;
      markCache = new Map();
    }
    let index = markCache.get(key);
    if (index === undefined) {
      const b = bundle();
      index = markerIndex(key, {
        def,
        src: (k) => b?.quest_src[k] ?? [],
        dst: (k) => b?.quest_dst[k] ?? [],
        subject,
        fixes: fixes(),
      });
      markCache.set(key, index);
    }
    return index;
  };

  const completable = (questId: number): boolean => {
    const d = def(questId);
    return (
      d !== undefined && findRecord(book(), questId) !== undefined && canEnd(d, subject, fixes())
    );
  };

  const joinTitles = (): string[] => {
    if (!available) return [];
    const out: string[] = [];
    for (const r of book().active) {
      if (NO_JOIN_LINE.has(r.id) || r.state === 14) continue;
      const d = def(r.id);
      if (d) out.push(d.title);
    }
    return out;
  };

  // ---- the gate in motion ----------------------------------------------------------------------------------------------------------

  let uiHooked = false;
  let factsHooked = false;

  /** The facts the lines and marks read (subscribed once, when the quests are available). */
  const hookFacts = (): void => {
    if (factsHooked) return;
    factsHooked = true;
    let selfKey = '';
    selfStore.subscribe((s) => {
      const key = `${String(s.level)}|${String(s.job)}|${String(s.sex)}|${String(s.gold)}|${String(s.expPercentX100)}|${String(s.pk.value)}|${String(s.partyId)}|${String(s.guildId)}`;
      if (key === selfKey) return;
      selfKey = key;
      bumpFacts();
    });
    const inv = inventoryOf(ctx);
    inv?.store.subscribe((s, prev) => {
      if (s.version !== prev.version) bumpFacts();
    });
    partyOf(ctx)?.store.subscribe(bumpFacts);
    skillTreeOf(ctx)?.store.subscribe(bumpFacts);
    petsOf(ctx)?.store.subscribe(bumpFacts);
    relationOf(ctx)?.store.subscribe(bumpFacts);
    guildOf(ctx)?.store.subscribe(bumpFacts);
  };

  /** Everything that needs the UI or the other features: the applet, the hooks, the quick info at field entry. */
  const ensureUi = (): void => {
    if (!available) return;
    const ui = fullUi(ctx);
    if (!ui) return;
    if (!uiHooked) {
      uiHooked = true;
      ui.quest.enableApplet(listPorts);
      dialogOf(ctx)?.setQuestLookup((id) => {
        const d = def(id);
        return d
          ? { title: d.title, levelMin: d.begin.level[0], levelMax: d.begin.level[1] }
          : null;
      });
      dialogOf(ctx)?.setCompletable(completable);
      navigatorOf(ctx)?.setQuestEmoticonSource(emoticonOf);
      hookFacts();
    }
    // `OpenField`: the quick info exists in the world (visible while a quest is ticked)
    if (ctx.machine.screen.s === 'InGame' && !ui.quest.isOpen(APP_QUEST_QUICK_INFO)) {
      ui.quest.openQuickInfo(quickRequest);
    }
  };

  /** The `quests` bundle is in: the gate decides, once. */
  const decide = (): void => {
    if (decided) return;
    const b = bundle();
    if (!b) return;
    decided = true;
    available = b.quests.length > 0;
    if (available) {
      ensureUi();
      bumpBook();
    }
  };

  /** Asks for the bundle once the session is logged in (a gated server refuses a lazy bundle before it). */
  const maybeEnsure = (): void => {
    attachData();
    if (decided) return;
    if (bundle()) {
      decide();
      return;
    }
    if (requested || !ctx.bundles.isLoggedIn) return;
    requested = true;
    ctx.bundles.ensure('quests').then(
      () => {
        decide();
      },
      () => {
        // a failed load is tried again at the next transition
        requested = false;
      },
    );
  };

  let dataHooked = false;
  const attachData = (): void => {
    const d = ctx.data;
    if (dataHooked || !d || typeof d.onBundle !== 'function') return;
    dataHooked = true;
    d.onBundle((name) => {
      if (name === 'quests') decide();
    });
  };

  ctx.machine.subscribe((screen, previous) => {
    maybeEnsure();
    ensureUi();
    // the session leaves the world: the quest windows go with the field (`closeField`), the memory of the windows is forgotten (`OpenTitle`: the quick info and the tree information are deleted)
    const leftWorld =
      (previous.s === 'InGame' || previous.s === 'Loading' || previous.s === 'Reconnecting') &&
      screen.s !== 'InGame' &&
      screen.s !== 'Loading' &&
      screen.s !== 'Reconnecting';
    if (leftWorld) {
      fullUi(ctx)?.quest.closeAll();
      const b = book();
      store.setState({ ...b, tree: { open: new Map(), tab: 0, group: 1 }, version: b.version + 1 });
      destination()?.destination.clear();
    }
  });
  ctx.bundles.subscribe(() => {
    maybeEnsure();
  });
  ctx.tickSystems.register('quest.ui', () => {
    if (!decided) maybeEnsure();
    ensureUi();
  });
  maybeEnsure();

  // ---- the input layer ---------------------------------------------------------------------------------------------------------------

  inputProvidersOf(ctx).questDestination = {
    active: () => destination()?.destination.fromNpc === true,
    clear: () => {
      destination()?.destination.clearNpcFlag();
    },
  };

  // ---- the public face -------------------------------------------------------------------------------------------------------------

  const feature: QuestFeature = {
    store,
    senders,
    available: () => available,
    def,
    record: (id) => findRecord(book(), id),
    emoticonOf,
    completable,
    joinTitles,
    onChange: onAny,
  };
  setQuestOf(ctx, feature);

  addProbeMember(ctx, 'quest', () => {
    const b = book();
    const ui = fullUi(ctx);
    const emoticons: Record<string, number> = {};
    ctx.entities.forEach(EntityKind.Npc, (rec) => {
      if (rec.removed) return;
      const key = (rec.spawn as SpawnNpc).npc?.characterKey ?? '';
      if (key === '') return;
      const index = emoticonOf(rec.objId);
      if (index >= 0) emoticons[key] = index;
    });
    return {
      available,
      count: bundle()?.quests.length ?? 0,
      ids: [...defs().keys()],
      appletAvailable: ui?.wm.applets.isAvailable(APP_QUEST_EX_LIST) === true,
      active: b.active.map((r) => ({
        id: r.id,
        state: r.state,
        timeLeft: r.timeLeft,
        frozen: r.frozen,
        kills: [...r.kills],
        patrol: r.patrol,
        dialog: r.dialog,
      })),
      completed: [...b.completed],
      checked: [...b.checked],
      guildQuests: [...b.guildQuests],
      textTime: b.textTime,
      tree: { open: [...b.tree.open], tab: b.tree.tab, group: b.tree.group },
      windows: {
        list: ui?.quest.list() !== null,
        detail: ui?.quest.detail()?.questId ?? null,
        quickInfo: ui?.quest.quickInfo() != null,
        heavenTower: ui?.quest.heavenTower() != null,
      },
      emoticons: () => emoticons,
      destination: destination()?.destination.get() ?? null,
      stats: { version },
    };
  });

  return feature;
}
