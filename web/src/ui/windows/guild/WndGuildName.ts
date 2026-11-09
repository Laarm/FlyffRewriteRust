// `WndGuildName` and `WndGuildNickName` (`CWndGuildName`, `CWndGuildNickName`, `src:_Interface/WndGuildName.cpp:20-333`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-45, -46): the two
// small naming windows of the guild.
//
//   APP_GUILDNAME (345, 272 x 128)    "Enter the guild name": an edit control (starts with the name the guild has, `''` while unnamed) with OK / Cancel. The window serves the
//                                     first naming by the master AND the rename with a scroll (the request tells which; the send is the caller's).
//   APP_GUILD_NICKNAME (349)          "Enter Title": the alias of a member; OK / Cancel.
//
// OK hands the trimmed text to the caller (`submit`), which runs the checks of the original in order (length -> `TID_DIAG_0011`, first character a digit -> `TID_DIAG_0012`,
// not ASCII alphanumeric -> `TID_DIAG_0013`, forbidden name -> `TID_DIAG_0020`), opens the message box of a refusal and sends; the window closes when the name was taken
// and stays when it was not. Cancel and the close button close.

import {
  APP_GUILD_NICKNAME,
  APP_GUILDNAME,
  WIDC_CANCEL,
  WIDC_EDIT1,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { NameRequest, NicknameRequest } from './ports';

abstract class WndNaming extends WndNeuz {
  readonly #request: NameRequest | NicknameRequest;
  #edit: EditCtrl | null = null;

  constructor(request: NameRequest | NicknameRequest) {
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
    this.#edit?.setString(this.#request.initial);
    this.moveParentCenter();
  }

  /** OK: the caller checks and sends; the window closes when the text was taken. */
  submit(): boolean {
    const text = (this.#edit?.getString() ?? '').trim();
    if (!this.#request.submit(text)) return false;
    this.destroy();
    return true;
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

export class WndGuildName extends WndNaming {
  readonly appId = APP_GUILDNAME;
}

export class WndGuildNickName extends WndNaming {
  readonly appId = APP_GUILD_NICKNAME;
}
