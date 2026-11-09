// The look of the quiz button (`CWndQuizEventButton::Process` / `OnDraw`, `src:_Interface/WndQuizEvent.cpp:264-456`; WP-UI-59), as pure functions: the pulse is a function of the elapsed time (the window manager's `process()`
// runs at 10 Hz, the original stepped `m_nAlpha` once per rendered frame), the sprite is a function of the state, the pointer and the press.

/** One tick of the pulse (1/60 s: the original's step is per frame; FIX: per tick of the 60 Hz clock). */
export const QUIZ_TICK_MS = 1000 / 60;
/** The alpha runs 255 -> 100 -> 255 in steps of 5: 31 ticks down, 31 ticks up. */
export const QUIZ_ALPHA_MAX = 255;
export const QUIZ_ALPHA_MIN = 100;
export const QUIZ_ALPHA_STEP = 5;
export const QUIZ_PULSE_TICKS = 62;

/** `m_nAlpha` after `ms` of pulsing (255 at 0, 100 after 31 ticks, 255 after 62, then again). */
export function quizButtonAlpha(ms: number): number {
  const tick = Math.max(0, Math.floor(ms / QUIZ_TICK_MS));
  const p = tick % QUIZ_PULSE_TICKS;
  const half = QUIZ_PULSE_TICKS / 2;
  return p <= half
    ? QUIZ_ALPHA_MAX - QUIZ_ALPHA_STEP * p
    : QUIZ_ALPHA_MIN + QUIZ_ALPHA_STEP * (p - half);
}

/** What the window draws: the sprite of `texMapQuizEventButton.inc` (0 bright, 2 dark) or nothing, and its alpha (255 = opaque). */
export interface QuizButtonPaint {
  readonly sprite: 0 | 2 | null;
  readonly alpha: number;
}

/**
 * `OnDraw`: QUESTION state - idle: sprite 0 with the pulsing alpha; hover: sprite 0; hover and pushed: sprite 2; pushed with the pointer outside: nothing (KEEP: `nTexNum = -1`). Any other state: sprite 2.
 */
export function quizButtonPaint(
  active: boolean,
  hover: boolean,
  pushed: boolean,
  ms: number,
): QuizButtonPaint {
  if (!active) return { sprite: 2, alpha: QUIZ_ALPHA_MAX };
  if (!hover && !pushed) return { sprite: 0, alpha: quizButtonAlpha(ms) };
  if (hover && !pushed) return { sprite: 0, alpha: QUIZ_ALPHA_MAX };
  if (hover && pushed) return { sprite: 2, alpha: QUIZ_ALPHA_MAX };
  return { sprite: null, alpha: QUIZ_ALPHA_MAX };
}

/** The atlas of the button (`Theme\texMapQuizEventButton.inc`). */
export const QUIZ_BUTTON_ATLAS = 'theme/texmapquizeventbutton.inc';
