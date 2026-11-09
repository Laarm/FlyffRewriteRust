// The quiz windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-59): the four layouts become `built`, all lazy.
import type { WndCtor } from '../../catalog';
import { QUIZ_LAZY } from './lazy';

export const QUIZ_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_QUIZ_BUTTON: () => QUIZ_LAZY.WndQuizEventButton.load(),
  APP_QUIZ_CONFIRM: () => QUIZ_LAZY.WndQuizEventConfirm.load(),
  APP_QUIZ_QUESTION_OX: () => QUIZ_LAZY.WndQuizEventQuestionOX.load(),
  APP_QUIZ_QUESTION_4C: () => QUIZ_LAZY.WndQuizEventQuestion4C.load(),
};
