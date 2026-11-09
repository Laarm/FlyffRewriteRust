// `WndAddFriend` (`CWndAddFriend`, `src:_Interface/WndFriendConFirm.cpp:110-230`, Plan/42 §19.4 I, 45 §24.1; WP-UI-43): `APP_ADDFRIEND` (240 x 144), "Enter Name" with one
// edit, OK (`WIDC_OK`) and Cancel (`WIDC_CANCEL`). OK hands the typed name to the request, which runs the checks of the original (the own name, 64 bytes, the guild-war
// world) and sends; it answers `clear` (the edit empties, the window stays) or `close`. The window is centred.

import { APP_ADDFRIEND, WIDC_CANCEL, WIDC_EDIT1, WIDC_OK } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { AddFriendRequest } from './ports';

export class WndAddFriend extends WndNeuz {
  readonly appId = APP_ADDFRIEND;
  readonly #request: AddFriendRequest;
  #edit: EditCtrl | null = null;

  constructor(request: AddFriendRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.setFocus();
    this.moveParentCenter();
  }

  /** OK: the request judges the text exactly as typed (no trimming: the original passed the edit's string on). */
  submit(): void {
    const answer = this.#request.submit(this.#edit?.getString() ?? '');
    if (answer === 'clear') this.#edit?.setString('');
    else this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.submit();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
