// `features/events/quiz`: the quiz event (the module is registered by `features/rainbowM7.ts`).
export { quizOf, registerQuiz } from './quizModule';
export type { QuizFeature } from './quizModule';
export { initialQuiz, QUIZ_STATE } from './state';
export type { QuizSlice, QuizType } from './state';
export { QUIZ_RECTS, ZONE, zoneKindOf } from './zones';
