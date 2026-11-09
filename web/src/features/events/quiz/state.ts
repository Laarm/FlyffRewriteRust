// The quiz slice of the events store (Plan/45 §28.0, §28.8; WP-CFEAT-44): `CQuiz::GetInstance()` as the client keeps it. Reset when the character leaves the world (the next `QuizState` comes at the entrance).
//
//   type      1 OX, 2 four choices, 0 none seen yet (a `QuizQuestion` before any `QuizState` fills it: FIX, quiz open question 3)
//   state     `QE_CLOSE` 0, `QE_OPEN` 1, `QE_CLOSE_WAIT` 2, `QE_WATCHINGZONE_OPEN` 3, `QE_QUESTION` 4, `QE_CORRECT_ANSWER` 5, `QE_DROP_OUT` 6, `QE_WINNER` 7
//   question  `m_strQuestion`: the lines of the question (and the choices of a 4C quiz), `\n`-separated
//   count     `m_nQCount`: the seconds left to answer (the countdown caption); the question message carries the answer time (15)
//   countAt   the clock (ms of `ctx.now()`) the next second of the countdown is counted from, null while no countdown runs

export type QuizType = 0 | 1 | 2;

export interface QuizSlice {
  readonly type: QuizType;
  readonly state: number;
  readonly question: string;
  readonly count: number;
  readonly countAt: number | null;
}

export function initialQuiz(): QuizSlice {
  return { type: 0, state: 0, question: '', count: 0, countAt: null };
}

/** `CQuiz::QE_*`. */
export const QUIZ_STATE = {
  Close: 0,
  Open: 1,
  CloseWait: 2,
  WatchingZoneOpen: 3,
  Question: 4,
  CorrectAnswer: 5,
  DropOut: 6,
  Winner: 7,
} as const;
