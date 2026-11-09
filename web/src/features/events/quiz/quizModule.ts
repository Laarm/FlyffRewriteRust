// The `quiz` feature (Plan/45 §28.8; WP-CFEAT-44; server half `event/quiz/`): the GM-started quiz event of world 211.
//
//   messages     `QuizState`, `QuizMessage`, `QuizQuestion`, `QuizNotice` -> `eventStore.quiz` (`reducers.ts`, pure); every message reduces whether or not a window is open
//   windows      the shells of `ui.quiz` (ui-house-quiz): the pulsing button (state QUESTION in world 211), the entrance / exit confirm, the OX and the 4C question windows; this feature calls them with requests
//   menu         137 (confirm, `QuizEntrance`), 138 (`QuizTeleport{ 100 }` in OPEN / WATCHINGZONE_OPEN), 139 (confirm, `QuizTeleport{ 106 }`)
//   answers      the buttons of the question windows send `QuizTeleport{ 101..104 }` only outside CORRECT_ANSWER / DROP_OUT and inside the quiz zone, else the chat line `TID_GAME_QUIZ_DO_NOT_TELEPORT`
//   captions     the countdown of the answer time (1 s cadence), the quiz captions and the announcements
//   probe        `__flyffDebug.features.quiz()`
//
// Not built (not in the C++): the Plan's "watching-zone button (105)" and "`APP_QUIZ_BUTTON` exit"; zone 105 is never sent by V15's client. No client gate and no config key: the quiz has no flag (PD-12, operator content).

import type { ClientContext } from '../../../app/context';
import { APP_QUIZ_BUTTON } from '../../../gen/defs/resdata';
import {
  TID_GAME_QUIZ_DO_NOT_TELEPORT,
  TID_GAME_QUIZ_TELEPORT_QUIZ_IN,
  TID_GAME_QUIZ_TELEPORT_QUIZ_OUT,
} from '../../../gen/defs/text';
import { WI_WORLD_QUIZ } from '../../../gen/defs/world';
import { CHATSTY } from '../../../ui/chat/styles';
import { fullUi, type FeatureContext } from '../../common/module';
import { sysChat } from '../../common/text';
import { hudOf } from '../../hud/hudModule';
import { selfPosition } from '../../navigator/runtime';
import { menuOf } from '../../npc/menuModule';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import type { EventStore } from '../store';
import { QUIZ_MENU_IDS, quizMenuTable } from './menu';
import { registerQuizProbe } from './probe';
import {
  type NoticeEnv,
  type QuizEffect,
  quizNoticeEffects,
  reduceLifecycle,
  reduceQuizMessage,
  reduceQuizQuestion,
  reduceQuizState,
} from './reducers';
import { quizSenders, type QuizSenders } from './senders';
import { initialQuiz, QUIZ_STATE, type QuizSlice } from './state';
import { ZONE, zoneKindOf } from './zones';

/** The public face of the quiz feature of a context. */
export interface QuizFeature {
  readonly store: EventStore;
  readonly senders: QuizSenders;
  /** The slice now. */
  state(): QuizSlice;
  /** The character stands in the quiz world. */
  inQuizWorld(): boolean;
  /** `GetZoneType( g_pPlayer )` of the own position. */
  zone(): 'quiz' | 'watching';
  /** The answer buttons of the question windows (101..104). */
  answer(zone: number): void;
  /** Runs the lifecycle (button, windows, countdown) now; the frame hook does it too. */
  evaluate(): void;
}

const features = new WeakMap<ClientContext, QuizFeature>();

/** The quiz feature of a context; `undefined` before the module registered. */
export function quizOf(ctx: ClientContext): QuizFeature | undefined {
  return features.get(ctx);
}

/** Registers the quiz (after the events feature: `rainbowM7.ts` registers the groups under the one handler module). */
export function registerQuiz(fc: FeatureContext): QuizFeature {
  const ctx = fc.app;
  const known = quizOf(ctx);
  if (known) return known;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const { worldStore, sessionStore } = ctx.stores;
  const senders = quizSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const slice = (): QuizSlice => store.getState().quiz;
  const set = (next: QuizSlice): void => {
    if (next !== store.getState().quiz) store.setState({ quiz: next });
  };
  const inQuizWorld = (): boolean => worldStore.getState().worldId === WI_WORLD_QUIZ;
  const zone = (): 'quiz' | 'watching' => {
    const pos = selfPosition(ctx);
    return pos ? zoneKindOf(slice().type, pos.x, pos.z) : 'watching';
  };

  // ---- the answers -------------------------------------------------------------------------------------------------------------------

  /** The chat line of a refused move: `TID_GAME_QUIZ_DO_NOT_TELEPORT` in its colour. */
  const refuse = (): void => {
    sysChat(fc.texts, fc.chat, TID_GAME_QUIZ_DO_NOT_TELEPORT);
  };
  const answer = (z: number): void => {
    const s = slice();
    if (
      s.state !== QUIZ_STATE.CorrectAnswer &&
      s.state !== QUIZ_STATE.DropOut &&
      zone() === 'quiz'
    ) {
      senders.teleport(z);
    } else refuse();
  };

  // ---- the windows -------------------------------------------------------------------------------------------------------------------

  const openQuestion = (type: 1 | 2, question: string): void => {
    const ui = fullUi(ctx)?.quiz;
    if (!ui) return;
    if (type === 1) {
      ui.openQuestionOx({
        question,
        onAnswer: (z) => {
          answer(z);
        },
      });
    } else {
      ui.openQuestion4c({
        question,
        onAnswer: (z) => {
          answer(z);
        },
      });
    }
  };

  /** The button: the window of the stored type with the stored question opens, or closes when it is open (`WndQuizEvent.cpp:334-384`). */
  const toggleQuestion = (): void => {
    const ui = fullUi(ctx)?.quiz;
    if (!ui) return;
    if (ui.isQuestionOpen()) {
      ui.closeQuestions();
      return;
    }
    const s = slice();
    openQuestion(s.type === 1 ? 1 : 2, s.question);
  };

  const run = (effects: readonly QuizEffect[]): void => {
    const ui = fullUi(ctx);
    for (const e of effects) {
      switch (e.kind) {
        case 'caption':
          hudOf(ctx).caption(e.text, e.small, ctx.now());
          break;
        case 'clearCaptions':
          hudOf(ctx).clearCaptions();
          break;
        case 'openQuestion':
          openQuestion(e.type, e.question);
          break;
        case 'closeQuestions':
          ui?.quiz.closeQuestions();
          break;
        case 'openButton':
          ui?.quiz.openButton({
            active: () => slice().state === QUIZ_STATE.Question,
            onClick: toggleQuestion,
          });
          break;
        case 'closeButton':
          ui?.quiz.closeButton();
          break;
        case 'announce':
          fc.chat?.putString(e.text, null, e.color, CHATSTY.SYSTEM);
          break;
      }
    }
  };

  // ---- the lifecycle: world, slice and clock ------------------------------------------------------------------------------------

  let evaluating = false;
  const evaluate = (): void => {
    if (evaluating) return;
    evaluating = true;
    try {
      const ui = fullUi(ctx)?.quiz;
      const r = reduceLifecycle(slice(), {
        inQuizWorld: inQuizWorld(),
        buttonOpen: ui?.isOpen(APP_QUIZ_BUTTON) ?? false,
        questionOpen: ui?.isQuestionOpen() ?? false,
        now: ctx.now(),
      });
      set(r.slice);
      run(r.effects);
    } finally {
      evaluating = false;
    }
  };
  store.subscribe((s, prev) => {
    if (s.quiz !== prev.quiz) evaluate();
  });
  worldStore.subscribe(evaluate);
  ctx.tickSystems.register('quiz.lifecycle', evaluate);

  // ---- the messages ---------------------------------------------------------------------------------------------------------------------

  fc.net.on('quizState', (m) => {
    set(reduceQuizState(slice(), { type: m.type, state: m.state }));
  });
  fc.net.on('quizQuestion', (m) => {
    const r = reduceQuizQuestion(
      slice(),
      { type: m.type, question: m.question, count: m.count },
      ctx.now(),
    );
    set(r.slice);
    run(r.effects);
  });
  fc.net.on('quizMessage', (m) => {
    const r = reduceQuizMessage(slice(), { message: m.message, state: m.state });
    set(r.slice);
    run(r.effects);
  });
  fc.net.on('quizNotice', (m) => {
    const env: NoticeEnv = {
      text,
      color: (tid) => fc.texts.textColor(tid),
      channelName: sessionStore.getState().server.name,
      inQuizWorld: inQuizWorld(),
    };
    run(quizNoticeEffects({ textId: m.textId, all: m.all, channel: m.channel, time: m.time }, env));
  });

  // the character leaves the world: the quiz is over for it (a world change keeps the slice: the quiz world IS a world change)
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    set(initialQuiz());
    fullUi(ctx)?.quiz.closeAll();
  });

  // ---- the NPC menu --------------------------------------------------------------------------------------------------------------------

  const table = quizMenuTable({
    confirmEntrance: () => {
      fullUi(ctx)?.quiz.openConfirm({
        text: text(TID_GAME_QUIZ_TELEPORT_QUIZ_IN),
        onYes: () => {
          senders.entrance();
        },
      });
    },
    teleportQuizZone: () => {
      const st = slice().state;
      if (st === QUIZ_STATE.Open || st === QUIZ_STATE.WatchingZoneOpen) senders.teleport(ZONE.Quiz);
      else refuse();
    },
    confirmExit: () => {
      fullUi(ctx)?.quiz.openConfirm({
        text: text(TID_GAME_QUIZ_TELEPORT_QUIZ_OUT),
        onYes: () => {
          senders.teleport(ZONE.Exit);
        },
      });
    },
  });
  for (const id of QUIZ_MENU_IDS) {
    const fn = table.get(id);
    if (fn)
      menuOf(ctx)?.registerAction(id, () => {
        fn();
      });
  }

  const feature: QuizFeature = {
    store,
    senders,
    state: slice,
    inQuizWorld,
    zone,
    answer,
    evaluate,
  };
  features.set(ctx, feature);
  registerQuizProbe(ctx, slice);
  return feature;
}
