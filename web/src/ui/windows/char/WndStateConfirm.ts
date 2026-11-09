// `WndStateConfirm` (`CWndStateConfirm`, `src:_Interface/WndField.cpp:4746-4910`, ref/22 §17 D, Plan/42 §5.6 R4, §19.4 D; WP-UI-21):
// `APP_STATE_CONFIRM` (304 x 128), the question "use the current stat setup?" before the allocated stat points are sent. A
// read-only text, Yes and No. Yes sends the allocation through the character window (the request); No and the close button
// cancel. This is the one window whose keys act even over another modal (`KillWindow`, `Neuz.cpp:1536`): the key router calls
// `yes()` for Enter and `cancel()` for Esc.

import { APP_STATE_CONFIRM, WIDC_NO, WIDC_TEXT1, WIDC_YES } from '../../../gen/defs/resdata';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { replaceWithText } from '../readOnlyText';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { StateConfirmRequest } from './ports';

export class WndStateConfirm extends WndNeuz {
  readonly appId = APP_STATE_CONFIRM;
  readonly #request: StateConfirmRequest;
  #done = false;

  constructor(request: StateConfirmRequest) {
    super();
    this.#request = request;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const text = replaceWithText(this, WIDC_TEXT1);
    text?.setString(this.#request.text);
    text?.enableWindow(false);
    this.moveParentCenter();
  }

  /** Yes / Enter: the request's `yes()` and the window closes. */
  yes(): void {
    if (this.#done) return;
    this.#done = true;
    this.#request.yes();
    this.destroy();
  }

  /** No / the close button / Esc: the request's `no()` and the window closes. */
  cancel(): void {
    if (this.#done) return;
    this.#done = true;
    this.#request.no();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.yes();
        return true;
      }
      if (id === WIDC_NO || id === WTBID_CLOSE) {
        this.cancel();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
