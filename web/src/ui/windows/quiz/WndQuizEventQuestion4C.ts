// `WndQuizEventQuestion4C` (`CWndQuizEventQuestion4C`, `src:_Interface/WndQuizEvent.cpp:160-261`, Plan/42 §19.4 P, 45 §28.8; WP-UI-59): `APP_QUIZ_QUESTION_4C` (640 x 240), the four-choice question window. `NO1..NO4` report the zones
// 101..104 and close, `CLOSE` closes. The first line of the question is the question, the next four the choices (the client of the original showed the whole text in `TEXT1`).

import {
  APP_QUIZ_QUESTION_4C,
  WIDC_NO1,
  WIDC_NO2,
  WIDC_NO3,
  WIDC_NO4,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import type { Quiz4cRequest, Quiz4cZone } from './ports';
import { QuizQuestionWnd } from './question';

const ZONES = new Map<number, Quiz4cZone>([
  [WIDC_NO1, 101],
  [WIDC_NO2, 102],
  [WIDC_NO3, 103],
  [WIDC_NO4, 104],
]);

export class WndQuizEventQuestion4C extends QuizQuestionWnd {
  readonly appId = APP_QUIZ_QUESTION_4C;
  readonly #request: Quiz4cRequest;

  constructor(request: Quiz4cRequest) {
    super(request.question, request.onClose?.bind(request));
    this.#request = request;
  }

  button(n: 1 | 2 | 3 | 4): Button | null {
    const ids = [WIDC_NO1, WIDC_NO2, WIDC_NO3, WIDC_NO4] as const;
    const id = ids[n - 1];
    return id === undefined ? null : this.getDlgItem<Button>(id);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const zone = ZONES.get(id);
    if (msg === Wnm.CLICKED && zone !== undefined) {
      this.answered(() => {
        this.#request.onAnswer(zone);
      });
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
