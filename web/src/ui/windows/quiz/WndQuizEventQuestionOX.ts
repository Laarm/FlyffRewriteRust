// `WndQuizEventQuestionOX` (`CWndQuizEventQuestionOX`, `src:_Interface/WndQuizEvent.cpp:78-157`, Plan/42 §19.4 P, 45 §28.8; WP-UI-59): `APP_QUIZ_QUESTION_OX` (592 x 240), the O / X question window. `O` and `X` report the zones
// 101 and 102 and close, `CLOSE` closes. The guards (`TID_GAME_QUIZ_DO_NOT_TELEPORT`) are the feature's.

import { APP_QUIZ_QUESTION_OX, WIDC_O, WIDC_X } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import type { QuizOxRequest } from './ports';
import { QuizQuestionWnd } from './question';

export class WndQuizEventQuestionOX extends QuizQuestionWnd {
  readonly appId = APP_QUIZ_QUESTION_OX;
  readonly #request: QuizOxRequest;

  constructor(request: QuizOxRequest) {
    super(request.question, request.onClose?.bind(request));
    this.#request = request;
  }

  get oButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_O);
  }
  get xButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_X);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && (id === WIDC_O || id === WIDC_X)) {
      this.answered(() => {
        this.#request.onAnswer(id === WIDC_O ? 101 : 102);
      });
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
