// `WndGuildWarRequest` (`CWndGuildWarRequest`, `src:_Interface/WndGuildWarRequest.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-48): `APP_GUILD_WARREQUEST` (288 x 128), the
// declaration of a war against the own guild (opened by `DeclGuildWarNtf`): "`<guild>` (`<master>`) declares war" in the disabled edit `WIDC_EDIT1`, Yes (`WIDC_YES`) and No
// (`WIDC_NO`). Yes -> `SendAcptWar( idMaster, idDecl )` (no client timer: the server drops a stale accept silently), No closes. Centred.

import { APP_GUILD_WARREQUEST, WIDC_EDIT1, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { WarRequestRequest } from './ports';

export class WndGuildWarRequest extends WndNeuz {
  readonly appId = APP_GUILD_WARREQUEST;
  readonly #request: WarRequestRequest;

  constructor(request: WarRequestRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const edit = this.edit;
    edit?.setString(this.#request.text);
    edit?.enableWindow(false);
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
