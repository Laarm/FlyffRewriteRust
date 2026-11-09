// `WndGuildDismiss` (`CWndGuildDisMiss`, `src:_Interface/WndGuildTabInfo.cpp:20-95`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-45): `APP_GUILD_DISMISS` (288 x 128), "Do you want to
// disband the guild?" with Yes (`WIDC_YES`) and No (`WIDC_NO`). A MODAL window, centred. The layout also has an edit control (`WIDC_EDIT1`) that the original never touches: it is
// built as the layout gives it and stays empty. Yes asks the caller to send `DestroyGuildReq` (the siege-application check is the caller's) and closes; No closes.

import { APP_GUILD_DISMISS, WIDC_EDIT1, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DismissRequest } from './ports';

export class WndGuildDismiss extends WndNeuz {
  readonly appId = APP_GUILD_DISMISS;
  readonly #request: DismissRequest;

  constructor(request: DismissRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `Create( WBS_MOVE | WBS_SOUND | WBS_CAPTION | WBS_MODAL ... )`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }
  /** The edit of the layout (never filled by the original). */
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.#request.yes();
        this.destroy();
        return true;
      }
      if (id === WIDC_NO) {
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
