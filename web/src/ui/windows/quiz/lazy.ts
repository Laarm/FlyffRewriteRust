// The quiz window shells in chunks of their own (`ui/wnd/lazy.ts`): the only place that imports them dynamically. One class per line (the vitest transform `lazyWndSync` reads this file line by line).
import { LazyWnd } from '../../wnd/lazy';

import type * as Button from './WndQuizEventButton';
import type * as Confirm from './WndQuizEventConfirm';
import type * as Question4c from './WndQuizEventQuestion4C';
import type * as QuestionOx from './WndQuizEventQuestionOX';

export const QUIZ_LAZY = {
  WndQuizEventButton: new LazyWnd<typeof Button.WndQuizEventButton>(
    'WndQuizEventButton',
    async () => (await import('./WndQuizEventButton')).WndQuizEventButton,
  ),
  WndQuizEventConfirm: new LazyWnd<typeof Confirm.WndQuizEventConfirm>(
    'WndQuizEventConfirm',
    async () => (await import('./WndQuizEventConfirm')).WndQuizEventConfirm,
  ),
  WndQuizEventQuestionOX: new LazyWnd<typeof QuestionOx.WndQuizEventQuestionOX>(
    'WndQuizEventQuestionOX',
    async () => (await import('./WndQuizEventQuestionOX')).WndQuizEventQuestionOX,
  ),
  WndQuizEventQuestion4C: new LazyWnd<typeof Question4c.WndQuizEventQuestion4C>(
    'WndQuizEventQuestion4C',
    async () => (await import('./WndQuizEventQuestion4C')).WndQuizEventQuestion4C,
  ),
} as const;
