// `WndGuildNotice` (`CWndGuildNotice`, `src:_Interface/WndGuildTabInfo.cpp:96-190`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-46): `APP_GUILD_NOTICE` (320 x 240), the notice of the
// guild. A MODAL window with one multi-line edit (`WIDC_EDIT1`, `EBS_WANTRETURN | EBS_AUTOVSCROLL`) that starts with the current notice and three buttons: `WIDC_BUTTON1` "New
// Notice" clears the text, `WIDC_BUTTON2` "Open Notice" sends it (when it is shorter than 128 bytes, else the caller opens `TID_GUILD_NOTICE_ERROR` and the window stays), `WIDC_BUTTON3`
// "Modify Notice" only gives the edit the focus. Centred.

import {
  APP_GUILD_NOTICE,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_EDIT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import {
  EBS_AUTOVSCROLL,
  EBS_MULTILINE,
  EBS_WANTRETURN,
  WBS_MODAL,
  Wnm,
} from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { NoticeRequest } from './ports';

export class WndGuildNotice extends WndNeuz {
  readonly appId = APP_GUILD_NOTICE;
  readonly #request: NoticeRequest;
  #edit: EditCtrl | null = null;

  constructor(request: NoticeRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  /** "New Notice". */
  get clearButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  /** "Open Notice": sends. */
  get sendButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  /** "Modify Notice": focuses the edit. */
  get focusButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON3);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    this.#edit?.addWndStyle(EBS_MULTILINE | EBS_WANTRETURN | EBS_AUTOVSCROLL);
    this.#edit?.setString(this.#request.initial);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      switch (id) {
        case WIDC_BUTTON2:
          if (this.#request.send(this.#edit?.getString() ?? '')) this.destroy();
          return true;
        case WIDC_BUTTON1:
          this.#edit?.setString('');
          return true;
        case WIDC_BUTTON3:
          this.#edit?.setFocus();
          return true;
        default:
          break;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
