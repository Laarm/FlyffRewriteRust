// The zones of the quiz world (Plan/22 §15.2; `Quiz.cpp:681-720` `InitQuizData` / `GetZoneType`; server `event/quiz/zones.rs`): the client asks them before it sends an answer (`WndQuizEvent.cpp:114, 125, 196`). The position
// is truncated toward zero and tested against `[left, right) x [top, bottom)` of the rect of the quiz type; a type without a rect (0: `m_sQuizData` zeroed) is the watching zone.

import type { QuizType } from './state';

/** `CQuiz::ZONE_*`: the zone numbers `QuizTeleport` carries (OX uses 101 and 102 only). */
export const ZONE = {
  Quiz: 100,
  One: 101,
  Two: 102,
  Three: 103,
  Four: 104,
  Watching: 105,
  Exit: 106,
} as const;

export interface QuizRect {
  readonly l: number;
  readonly t: number;
  readonly r: number;
  readonly b: number;
}

/** `QuizEventRect` of the OX quiz and of the four-choice quiz. */
export const QUIZ_RECTS: Readonly<Record<1 | 2, QuizRect>> = {
  1: { l: 768, t: 736, r: 832, b: 768 },
  2: { l: 1728, t: 1792, r: 1824, b: 1888 },
};

export type ZoneKind = 'quiz' | 'watching';

/** `GetZoneType( pPlayer )`: `ZONE_QUIZ` inside the rect of the quiz type, else `ZONE_WATCHING`. */
export function zoneKindOf(type: QuizType, x: number, z: number): ZoneKind {
  const rect = type === 0 ? undefined : QUIZ_RECTS[type];
  if (!rect) return 'watching';
  const px = Math.trunc(x);
  const pz = Math.trunc(z);
  return px >= rect.l && px < rect.r && pz >= rect.t && pz < rect.b ? 'quiz' : 'watching';
}
