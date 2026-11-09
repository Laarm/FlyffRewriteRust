// `QuizWindowsService` (`ui.quiz`, Plan/42 §19.4 P, 45 §28.8; WP-UI-59): the four window SHELLS of the quiz event. The quiz FEATURE (the store slice, the menus 137-139, the captions, the countdown, the button's visibility) is
// ui-rainbow's (WP-CFEAT-44), which calls exactly these methods:
//
//   ui.quiz.openButton(request) / closeButton()   APP_QUIZ_BUTTON: the pulsing button (replaces an open one)
//   ui.quiz.openConfirm(request)                  APP_QUIZ_CONFIRM: "enter / leave the quiz?"
//   ui.quiz.openQuestionOx(request)               APP_QUIZ_QUESTION_OX: closes the inventory; replaces an open question window of either kind
//   ui.quiz.openQuestion4c(request)               APP_QUIZ_QUESTION_4C: the same for four choices
//   ui.quiz.update(question)                      `UpdateQuestion`: the text of the open question window
//   ui.quiz.closeQuestions() / isQuestionOpen()   the question window of either kind goes / is open (the button's click toggles through these)
//   ui.quiz.isOpen(app) / closeAll()
//
// Contract notes: `onClose` of a question request runs when the window goes WITHOUT an answer (the CLOSE button, `closeQuestions()`, a replacement, the window manager); an answer calls `onAnswer` only.

import {
  APP_QUIZ_BUTTON,
  APP_QUIZ_CONFIRM,
  APP_QUIZ_QUESTION_4C,
  APP_QUIZ_QUESTION_OX,
} from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { QUIZ_LAZY } from './lazy';
import type { Quiz4cRequest, QuizButtonRequest, QuizConfirmRequest, QuizOxRequest } from './ports';
import type { WndQuizEventButton } from './WndQuizEventButton';
import type { WndQuizEventConfirm } from './WndQuizEventConfirm';
import type { WndQuizEventQuestion4C } from './WndQuizEventQuestion4C';
import type { WndQuizEventQuestionOX } from './WndQuizEventQuestionOX';

export * from './ports';
export * from './paint';
export type {
  WndQuizEventButton,
  WndQuizEventConfirm,
  WndQuizEventQuestion4C,
  WndQuizEventQuestionOX,
};

export interface QuizWindowDeps {
  readonly wm: WindowManager;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

/** The `APP_*` of every window of the family. */
export const QUIZ_APPS: readonly number[] = [
  APP_QUIZ_BUTTON,
  APP_QUIZ_CONFIRM,
  APP_QUIZ_QUESTION_OX,
  APP_QUIZ_QUESTION_4C,
];

export class QuizWindowsService {
  readonly #wm: WindowManager;
  readonly #atlases: UiAtlases;

  constructor(deps: QuizWindowDeps) {
    this.#wm = deps.wm;
    this.#atlases = deps.atlases;
  }

  #live(app: number): WndNeuz | null {
    const w = this.#wm.get(app);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `SAFE_DELETE( pWnd )`: the old window of the app goes at once (its `onDestroy` reports a close). */
  #replaceApp(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replaceApp(a);
    });
  }

  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>): InstanceType<C> | null {
    const w = this.#live(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  // ---- the button --------------------------------------------------------------------------------------------------------------

  openButton(request: QuizButtonRequest): WndQuizEventButton | null {
    return this.#open(
      APP_QUIZ_BUTTON,
      QUIZ_LAZY.WndQuizEventButton,
      (C) => new C(request, { sprite: (atlas, i) => this.#atlases.sprite(atlas, i) }),
    );
  }
  button(): WndQuizEventButton | null {
    return this.#get(APP_QUIZ_BUTTON, QUIZ_LAZY.WndQuizEventButton);
  }
  closeButton(): void {
    this.#live(APP_QUIZ_BUTTON)?.destroy();
  }

  // ---- the confirm and the question windows -----------------------------------------------------------------------------

  openConfirm(request: QuizConfirmRequest): WndQuizEventConfirm | null {
    return this.#open(APP_QUIZ_CONFIRM, QUIZ_LAZY.WndQuizEventConfirm, (C) => new C(request));
  }
  confirm(): WndQuizEventConfirm | null {
    return this.#get(APP_QUIZ_CONFIRM, QUIZ_LAZY.WndQuizEventConfirm);
  }

  /** A question window of the OTHER kind goes first (the original had one object per kind, and the feature opens the one of the quiz type). */
  openQuestionOx(request: QuizOxRequest): WndQuizEventQuestionOX | null {
    this.#replaceApp(APP_QUIZ_QUESTION_4C);
    return this.#open(
      APP_QUIZ_QUESTION_OX,
      QUIZ_LAZY.WndQuizEventQuestionOX,
      (C) => new C(request),
    );
  }
  questionOx(): WndQuizEventQuestionOX | null {
    return this.#get(APP_QUIZ_QUESTION_OX, QUIZ_LAZY.WndQuizEventQuestionOX);
  }

  openQuestion4c(request: Quiz4cRequest): WndQuizEventQuestion4C | null {
    this.#replaceApp(APP_QUIZ_QUESTION_OX);
    return this.#open(
      APP_QUIZ_QUESTION_4C,
      QUIZ_LAZY.WndQuizEventQuestion4C,
      (C) => new C(request),
    );
  }
  question4c(): WndQuizEventQuestion4C | null {
    return this.#get(APP_QUIZ_QUESTION_4C, QUIZ_LAZY.WndQuizEventQuestion4C);
  }

  /** `UpdateQuestion( text )` of the open question window. */
  update(question: string): void {
    this.questionOx()?.update(question);
    this.question4c()?.update(question);
  }

  isQuestionOpen(): boolean {
    return this.isOpen(APP_QUIZ_QUESTION_OX) || this.isOpen(APP_QUIZ_QUESTION_4C);
  }

  closeQuestions(): void {
    this.#live(APP_QUIZ_QUESTION_OX)?.destroy();
    this.#live(APP_QUIZ_QUESTION_4C)?.destroy();
  }

  /** Every quiz window goes (the session left the world). */
  closeAll(): void {
    for (const app of QUIZ_APPS) this.#live(app)?.destroy();
  }
}
