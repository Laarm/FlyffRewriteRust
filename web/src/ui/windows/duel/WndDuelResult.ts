// `WndDuelResult` (`CWndDuelResult`, `src:_Interface/WndDuelConfirm.cpp:380-455`, ref/22 §17 K, Plan/42 §19.4 K, 45 §25.1; WP-UI-41):
// `APP_DUEL_RESULT` (288 x 128), the end of a duel. One layout, two texts: "`<opponent>` loses the Duel!" for the winner and "`<opponent>`
// wins the Duel!" for the loser (`TID_DIAG_DUELRESULT_WIN` / `_LOSE` take the opponent's name). It is a normal (not modal) window,
// centred horizontally 70 px from the top. OK (`WIDC_YES`) or the title bar's close button ends it; the motion of the character
// (`MTI_HIPKIDDING` once for the winner, `MTI_HEAD` looped for the loser) and the movement report are the caller's `done()`.
// (The original reacts to EVERY child notification, mouse moves included; here only the two buttons end the window.)

import { APP_DUEL_RESULT, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { DUEL_WINDOW_Y } from './geometry';
import type { DuelResultRequest } from './ports';

export class WndDuelResult extends WndNeuz {
  readonly appId = APP_DUEL_RESULT;
  readonly #request: DuelResultRequest;
  #done = false;
  #text: TextCtrl | null = null;

  constructor(request: DuelResultRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  /** The own character won (the text and the motion differ). */
  get win(): boolean {
    return this.#request.win;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#request.text);
    this.#text?.enableWindow(false);
    const layout = this.env.wm.layoutRect;
    this.move({
      x: Math.trunc((layout.r - (this.rectWindow.r - this.rectWindow.l)) / 2),
      y: DUEL_WINDOW_Y,
    });
  }

  /** OK / the close button: the request's `done()` (the victory or defeat motion), then the window closes. */
  finish(): void {
    if (this.#done) return;
    this.#done = true;
    this.#request.done();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && (id === WIDC_YES || id === WTBID_CLOSE)) {
      this.finish();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
