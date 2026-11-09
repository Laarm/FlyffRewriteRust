// `WndRRMiniGameTyping` (`CWndRRMiniGameTyping`, `src:_Interface/WndRainbowRace.cpp:1853-1936`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-62): `APP_RR_MINIGAME_TYPING` (799, 384 x 416), "Typo!". `STATIC1-3` show the three
// questions (20 characters each), `EDIT1-3` take the answers (at most 255 characters: the server refuses strings over 256 bytes), `TEXT4` holds the panel, OK sends the three strings in the ext packet.
// FIX: OK is disabled after a send until the answer arrives (FAIL clears the edits and enables it again; FINISH closes the window).

import {
  APP_RR_MINIGAME_TYPING,
  WIDC_EDIT1,
  WIDC_EDIT2,
  WIDC_EDIT3,
  WIDC_OK,
  WIDC_STATIC1,
  WIDC_STATIC2,
  WIDC_STATIC3,
  WIDC_TEXT4,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { TypingRequest } from './ports';

const QUESTIONS = [WIDC_STATIC1, WIDC_STATIC2, WIDC_STATIC3] as const;
const EDITS = [WIDC_EDIT1, WIDC_EDIT2, WIDC_EDIT3] as const;
/** `setMaxLength( 255 )`. */
export const TYPING_MAX = 255;

export class WndRRMiniGameTyping extends WndNeuz {
  readonly appId = APP_RR_MINIGAME_TYPING;
  readonly #request: TypingRequest;

  constructor(request: TypingRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  edit(i: number): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(EDITS[i] ?? 0);
  }
  /** The question shown at line `i`. */
  questionText(i: number): string {
    return this.getDlgItem<Static>(QUESTIONS[i] ?? 0)?.state.get().text ?? '';
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<TextCtrl>(WIDC_TEXT4)?.setString(this.#request.text);
    for (let i = 0; i < 3; i += 1) this.edit(i)?.setMaxLength(TYPING_MAX);
    // the layout carries sample captions ("Edit", "...")
    for (let i = 0; i < 3; i += 1) {
      this.edit(i)?.setString('');
      this.getDlgItem<Static>(QUESTIONS[i] ?? 0)?.setTitle('');
    }
    (this.#request.questions ?? []).forEach((q, i) => {
      this.setQuestion(i, q);
    });
    this.moveParentCenter();
  }

  /** `SetQuestion( i, text )` (only 0..2). */
  setQuestion(i: number, text: string): void {
    if (i < 0 || i >= 3) return;
    this.getDlgItem<Static>(QUESTIONS[i] ?? 0)?.setTitle(text);
  }

  /** `ResetQuestion`: the three edits are empty; the answer button works again. */
  reset(): void {
    for (let i = 0; i < 3; i += 1) this.edit(i)?.setString('');
    this.okButton?.enableWindow(true);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      const answers = [0, 1, 2].map((i) => this.edit(i)?.getString() ?? '');
      this.okButton?.enableWindow(false);
      this.#request.submit(answers);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
