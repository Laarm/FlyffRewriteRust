// The reducers of the quiz (Plan/45 §28.8, ref/09b §3.16 "Quiz event"; `DPClient.cpp:18901-19075`, `WndWorld.cpp:9204-9245`; WP-CFEAT-44): PURE functions from the slice and a message to the new slice and a list of effects
// (data that the module carries out). Every message reduces whether or not a window is open.
//
//   reduceQuizState      `QuizState{ type, state }`: the type, the state, `count = 0` (`InitQuizData`, `m_nQCount = 0`)
//   reduceQuizQuestion   `QuizQuestion{ type, question, count }`: stores the question, opens the OX or the 4C window, starts the countdown
//   reduceQuizMessage    `QuizMessage{ message, state }`: the small caption; a state other than 0 is stored; 5 clears the question and closes both windows
//   quizNoticeEffects    `QuizNotice{ text_id, all, channel, time }`: the announcement line and the big caption
//   reduceLifecycle      the per-frame block of `CWndWorld::Process`: the button exists in world 211 during QUESTION only; the countdown steps once per second

import { TID_ADMIN_ANNOUNCE } from '../../../gen/defs/text';
import { formatTemplate } from '../../common/text';
import { QUIZ_STATE, type QuizSlice, type QuizType } from './state';

export type QuizEffect =
  /** A caption; `small` is the `m_pFontAPICaption` one, else the `m_pFontAPITitle` one. */
  | { readonly kind: 'caption'; readonly text: string; readonly small: boolean }
  /** `g_Caption1.RemoveAll()`. */
  | { readonly kind: 'clearCaptions' }
  /** Opens (replacing) the OX (type 1) or the 4C (type 2) question window. */
  | { readonly kind: 'openQuestion'; readonly type: 1 | 2; readonly question: string }
  | { readonly kind: 'closeQuestions' }
  | { readonly kind: 'openButton' }
  | { readonly kind: 'closeButton' }
  /** A system chat line of the announcement, in the colour of `TID_ADMIN_ANNOUNCE`. */
  | { readonly kind: 'announce'; readonly text: string; readonly color: number };

export interface Reduced {
  readonly slice: QuizSlice;
  readonly effects: QuizEffect[];
}

const asType = (n: number): QuizType => (n === 1 ? 1 : n === 2 ? 2 : 0);

/** `OnQuizEventState`. */
export function reduceQuizState(s: QuizSlice, m: { type: number; state: number }): QuizSlice {
  return { ...s, type: asType(m.type), state: m.state, count: 0, countAt: null };
}

/**
 * `OnQuizQuestion`: the window of the STORED type opens (OX for 1, else 4C). FIX: with no `QuizState` seen (stored type 0) the type of the message is used, and it is stored: the zone rects of the
 * answer test read it. The countdown starts from the message (`count` = the answer seconds).
 */
export function reduceQuizQuestion(
  s: QuizSlice,
  m: { type: number; question: string; count: number },
  now: number,
): Reduced {
  const type: QuizType = s.type !== 0 ? s.type : asType(m.type);
  const slice: QuizSlice = {
    ...s,
    type,
    question: m.question,
    count: m.count,
    countAt: m.count > 0 ? now : null,
  };
  return {
    slice,
    effects: [{ kind: 'openQuestion', type: type === 1 ? 1 : 2, question: m.question }],
  };
}

/** `OnQuizEventMessage`. */
export function reduceQuizMessage(s: QuizSlice, m: { message: string; state: number }): Reduced {
  const effects: QuizEffect[] = [{ kind: 'caption', text: m.message, small: true }];
  if (m.state === 0) return { slice: s, effects };
  if (m.state === QUIZ_STATE.CorrectAnswer) {
    effects.push({ kind: 'closeQuestions' });
    return { slice: { ...s, state: m.state, question: '', count: 0, countAt: null }, effects };
  }
  return { slice: { ...s, state: m.state }, effects };
}

/** What the notice needs of the client. */
export interface NoticeEnv {
  readonly text: (tid: number) => string;
  readonly color: (tid: number) => number;
  /** The name of the channel (`g_dpCertified.m_aServerset[ .. ].lpName`; one channel, D1). */
  readonly channelName: string;
  /** `g_pPlayer->GetWorld()->GetID() == WI_WORLD_QUIZ`. */
  readonly inQuizWorld: boolean;
}

/**
 * `OnQuizSystemMessage`: the text (with the channel name and the minutes when the message carries a channel), the system chat line `TID_ADMIN_ANNOUNCE( text )` and a big caption when `all` is set or the player is
 * not in the quiz world (the pale-yellow tint of the original is not in `HudState.caption`: default colour, NOTES).
 */
export function quizNoticeEffects(
  m: { textId: number; all: boolean; channel: number; time: number },
  env: NoticeEnv,
): QuizEffect[] {
  const template = env.text(m.textId);
  const body =
    m.channel !== 0
      ? formatTemplate(template, m.time !== 0 ? [env.channelName, m.time] : [env.channelName])
      : template;
  const line = formatTemplate(env.text(TID_ADMIN_ANNOUNCE), [body]);
  const effects: QuizEffect[] = [
    { kind: 'announce', text: line, color: env.color(TID_ADMIN_ANNOUNCE) },
  ];
  if (m.all || !env.inQuizWorld) effects.push({ kind: 'caption', text: line, small: false });
  return effects;
}

export interface LifecycleEnv {
  readonly inQuizWorld: boolean;
  readonly buttonOpen: boolean;
  readonly questionOpen: boolean;
  readonly now: number;
}

/**
 * The per-frame block of `CWndWorld::Process` (`WndWorld.cpp:9204-9245`): in world 211 the quiz button exists exactly during QUESTION (any other state closes the button and both question windows); outside it
 * everything closes. The countdown (world 211, QUESTION, `count > 0`) steps once per 1 000 ms after `countAt` (FIX: the original stepped every 64 frames, about 1.07 s, and drifted past the server's 15 s): the big
 * caption shows the digit, 0 only clears.
 */
export function reduceLifecycle(s: QuizSlice, env: LifecycleEnv): Reduced {
  const effects: QuizEffect[] = [];
  if (!env.inQuizWorld) {
    if (env.buttonOpen) effects.push({ kind: 'closeButton' });
    if (env.questionOpen) effects.push({ kind: 'closeQuestions' });
    return { slice: s, effects };
  }
  if (s.state !== QUIZ_STATE.Question) {
    if (env.buttonOpen) effects.push({ kind: 'closeButton' });
    if (env.questionOpen) effects.push({ kind: 'closeQuestions' });
    return { slice: s, effects };
  }
  if (!env.buttonOpen) effects.push({ kind: 'openButton' });
  if (s.count > 0 && s.countAt !== null) {
    const steps = Math.floor((env.now - s.countAt) / 1000);
    if (steps > 0) {
      const count = Math.max(0, s.count - steps);
      effects.push({ kind: 'clearCaptions' });
      if (count > 0) effects.push({ kind: 'caption', text: String(count), small: false });
      return {
        slice: { ...s, count, countAt: count > 0 ? s.countAt + steps * 1000 : null },
        effects,
      };
    }
  }
  return { slice: s, effects };
}
