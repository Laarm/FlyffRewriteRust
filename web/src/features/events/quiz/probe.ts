// The probe of the quiz (Plan/45 §2.6): `__flyffDebug.features.quiz()` reads the slice, the zone of the own position and the windows.

import type { ClientContext } from '../../../app/context';
import { WI_WORLD_QUIZ } from '../../../gen/defs/world';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { selfPosition } from '../../navigator/runtime';
import type { QuizSlice } from './state';
import { zoneKindOf } from './zones';

export function registerQuizProbe(ctx: ClientContext, slice: () => QuizSlice): void {
  addProbeMember(ctx, 'quiz', () => {
    const s = slice();
    const ui = fullUi(ctx)?.quiz;
    const pos = selfPosition(ctx);
    return {
      type: s.type,
      state: s.state,
      question: s.question,
      count: s.count,
      inQuizWorld: ctx.stores.worldStore.getState().worldId === WI_WORLD_QUIZ,
      zone: pos ? zoneKindOf(s.type, pos.x, pos.z) : null,
      buttonOpen: ui?.isOpen(839) ?? false,
      questionOpen: ui?.isQuestionOpen() ?? false,
    };
  });
}
