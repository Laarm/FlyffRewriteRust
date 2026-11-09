// The requests of the quiz window shells (Plan/42 §19.4 P, 45 §28.8; WP-UI-59). The windows only show and report: what the answers send (`QuizEntrance`, `QuizTeleport{ 100..106 }`), the guards of the answer
// (`TID_GAME_QUIZ_DO_NOT_TELEPORT`: the state is not CORRECT_ANSWER / DROP_OUT and the player stands in the quiz zone) and the quiz feature itself (the store slice, the menus 137-139, the captions, the countdown, the
// button's visibility) are ui-rainbow's (WP-CFEAT-44). The send table of CA3 (App. B names):
//
//   WndQuizEventButton        a press and a release inside      `onClick` (ui-rainbow toggles the question window of the quiz type)
//   WndQuizEventConfirm       YES                               `onYes` (`SendQuizEventEntrance` or `SendQuizEventTeleport( ZONE_EXIT )`), NO closes
//   WndQuizEventQuestionOX    O / X                             `onAnswer( 101 | 102 )` = `SendQuizEventTeleport( ZONE_1 | ZONE_2 )`; CLOSE closes
//   WndQuizEventQuestion4C    NO1..NO4                          `onAnswer( 101..104 )` = `SendQuizEventTeleport( ZONE_1..ZONE_4 )`; CLOSE closes

/** The answer zones (`CQuiz::ZONE_1..ZONE_4` of `zones.rs`: 101..104). */
export type QuizOxZone = 101 | 102;
export type Quiz4cZone = 101 | 102 | 103 | 104;

/** `APP_QUIZ_BUTTON`: the pulsing button that opens the question window. */
export interface QuizButtonRequest {
  /** The quiz state is QUESTION (`CQuiz::QE_QUESTION`): the button pulses and answers clicks; else it shows the dark sprite. */
  active(): boolean;
  /** A press and a release inside the window (`OnLButtonUp`). */
  onClick(): void;
}

/** `APP_QUIZ_CONFIRM`: "enter the quiz?" / "leave the quiz?". */
export interface QuizConfirmRequest {
  /** The text with the markup of the texts bundle (`AddParsingString`). */
  readonly text: string;
  /** YES: the window closes after the call. */
  onYes(): void;
}

/** `APP_QUIZ_QUESTION_OX`. */
export interface QuizOxRequest {
  /** The question; lines are `\n`-separated. */
  readonly question: string;
  /** O or X: the window closes after the call. */
  onAnswer(zone: QuizOxZone): void;
  /** The window went without an answer (`CLOSE`, `closeQuestions()`, the window manager). */
  onClose?(): void;
}

/** `APP_QUIZ_QUESTION_4C`: the question and the four choices (lines `\n`-separated). */
export interface Quiz4cRequest {
  readonly question: string;
  onAnswer(zone: Quiz4cZone): void;
  onClose?(): void;
}
