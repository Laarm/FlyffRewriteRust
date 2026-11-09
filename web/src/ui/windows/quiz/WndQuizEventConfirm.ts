// `WndQuizEventConfirm` (`CWndQuizEventConfirm`, `src:_Interface/WndQuizEvent.cpp:18-75`, Plan/42 §19.4 P, 45 §28.8; WP-UI-59): `APP_QUIZ_CONFIRM` (320 x 176), the yes / no question at the entrance and the exit of the quiz zone. `TEXT1` shows the
// text with its markup, `YES` reports and closes, `NO` closes. Centred.

import { APP_QUIZ_CONFIRM, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QuizConfirmRequest } from './ports';

export class WndQuizEventConfirm extends WndNeuz {
  readonly appId = APP_QUIZ_CONFIRM;
  readonly #request: QuizConfirmRequest;

  constructor(request: QuizConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setParsingString(this.#request.text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.#request.onYes();
        this.destroy();
        return true;
      }
      if (id === WIDC_NO) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
