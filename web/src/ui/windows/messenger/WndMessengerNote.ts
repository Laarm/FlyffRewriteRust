// `WndMessengerNote` (`CWndMessengerNote`, `src:_Interface/WndMessenger.cpp:1360-1438`, Plan/42 §19.4 I, 45 §24.1; WP-UI-43): `APP_MESSENGER_NOTE` (304 x 304), the notes the
// player was left while offline: `<from> - YYYY / MM / DD` in brown, the text indented, a blank line (`noteText.ts`). Opens once.

import { APP_MESSENGER_NOTE, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import { DEFAULT_TEXT_COLOR } from '../../text/markup';
import { WndNeuz } from '../../wnd/WndNeuz';
import { notePieces } from './noteText';
import type { NotesRequest } from './ports';

export class WndMessengerNote extends WndNeuz {
  readonly appId = APP_MESSENGER_NOTE;
  readonly #request: NotesRequest;
  #text: TextCtrl | null = null;

  constructor(request: NotesRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text = text;
    if (text) {
      for (const note of this.#request.notes) {
        for (const p of notePieces(note)) {
          text.addString(p.text, p.color ?? DEFAULT_TEXT_COLOR, p.macro);
        }
      }
    }
    this.moveParentCenter();
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
