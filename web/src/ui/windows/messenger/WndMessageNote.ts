// `WndMessageNote` (`CWndMessageNote`, `src:_Interface/WndMessenger.cpp:1260-1360`, Plan/42 §19.4 I, 45 §24.1; WP-UI-43): `APP_MESSAGE_NOTE` (336 x 96), the note to an
// offline friend. The title is the layout's title and " - <name>"; the edit takes several lines (`EBS_WANTRETURN`); the button sends the text to the request and the
// window closes, whatever the request made of it (the 255-byte rule and its line are the request's).

import { APP_MESSAGE_NOTE, WIDC_BUTTON1, WIDC_EDIT1 } from '../../../gen/defs/resdata';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { EBS_MULTILINE, EBS_WANTRETURN, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { NoteRequest } from './ports';

export class WndMessageNote extends WndNeuz {
  readonly appId = APP_MESSAGE_NOTE;
  readonly #request: NoteRequest;
  #edit: EditCtrl | null = null;

  constructor(request: NoteRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.addWndStyle(EBS_MULTILINE | EBS_WANTRETURN);
    this.#edit?.setFocus();
    this.setTitle(`${this.getTitle()} - ${this.#request.name}`);
    this.moveParentCenter();
  }

  /** The button: the text goes to the request, the window closes. */
  send(): void {
    this.#request.send(this.#edit?.getString() ?? '');
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.send();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
