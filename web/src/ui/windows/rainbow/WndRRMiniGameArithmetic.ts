// `WndRRMiniGameArithmetic` (`CWndRRMiniGameArithmetic`, `src:_Interface/WndRainbowRace.cpp:1545-1672`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-61): `APP_RR_MINIGAME_ARITHMETIC` (797, 272 x 240), "Anything but Math!".
// `TEXT1` shows the question, `EDIT1` takes the answer (free text: the minus sign must be typeable), `BUTTON1` sends `atoi( edit )`, `STATIC3` counts the correct answers in a row, `TEXT2` holds the panel.
// `setQuestion( q, n )` sets the question, clears the edit and enables the button again. NO client timer: the 10 s timer of the original is commented out in V15 and its time-over box is never opened
// (acceptance, `cfeat_44_arithmetic_has_no_timer`). FIX: `BUTTON1` is disabled after a send until the next `setQuestion` (a double click answered the new question with the old number and reset the streak).

import {
  APP_RR_MINIGAME_ARITHMETIC,
  WIDC_BUTTON1,
  WIDC_EDIT1,
  WIDC_STATIC3,
  WIDC_TEXT1,
  WIDC_TEXT2,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { atoi } from './atoi';
import type { ArithmeticRequest } from './ports';

export class WndRRMiniGameArithmetic extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_ARITHMETIC;
  readonly #request: ArithmeticRequest;
  #question = '';
  #correct = 0;

  constructor(request: ArithmeticRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get question(): string {
    return this.#question;
  }
  get correctCount(): number {
    return this.#correct;
  }
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }
  get answerButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get countStatic(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC3);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT2)?.setString(this.#request.text);
    if (this.#request.question !== undefined) {
      this.setQuestion(this.#request.question, this.#request.count ?? 0);
    }
    this.moveParentCenter();
  }

  /** `SetNextQuestion( question, correctCount )`. */
  setQuestion(question: string, correct: number): void {
    this.#question = question;
    this.#correct = correct;
    this.getDlgItem<TextCtrl>(WIDC_TEXT1)?.setString(question);
    this.answerButton?.enableWindow(true);
    this.edit?.setString('');
    this.countStatic?.setTitle(String(correct));
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      const n = atoi(this.edit?.getString() ?? '');
      this.answerButton?.enableWindow(false);
      this.#request.answer(n);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
