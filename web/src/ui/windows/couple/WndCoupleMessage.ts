// `WndCoupleMessage` (`CWndCoupleMessage`, `src:_Interface/WndField.cpp:23700-23775`, Plan/42 §14, 45 §24.2; WP-UI-52): a `CWndMessageBox( MB_OKCANCEL )` on `APP_MESSAGEBOX` of its own (instance
// key `'couple'`, so that `ui.msgbox.open` and the couple box never replace each other, like `g_WndMng.m_pWndCoupleMessage`). The three modes of the original - the proposal made by the
// player (`CM_SENDPROPOSE`), the proposal received (`CM_RECEIVEPROPOSE`) and the question before a break-up (`CM_CANCELCOUPLE`) - differ only in the text and in the callbacks the feature gives:
// OK and Cancel answer and close the box; the close button of the title bar answers nothing.
//
// A lazy chunk (`lazy.ts`): nothing imports this module as a value.

import { MessageBoxWnd } from '../../msgbox/MessageBoxWnd';
import { MB, MBID } from '../../msgbox/types';
import { COUPLE_MESSAGE_KEY, type CoupleMessageRequest } from './ports';

export class WndCoupleMessage extends MessageBoxWnd {
  override readonly instanceKey: string = COUPLE_MESSAGE_KEY;
  readonly #request: CoupleMessageRequest;

  constructor(request: CoupleMessageRequest) {
    super(request.text, MB.OKCANCEL);
    this.#request = request;
    this.putRegInfo = false;
    this.onResult = (id) => {
      if (id === MBID.OK) request.ok();
      else if (id === MBID.CANCEL) request.cancel?.();
    };
  }

  /** The text of the question. */
  get text(): string {
    return this.#request.text;
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
