// `WndGuildWarDecl` (`CWndGuildWarDecl`, `src:_Interface/WndGuildWarDecl.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-48): `APP_GUILD_WAR` (288 x 144), the declaration of a guild
// war: the target guild name (`WIDC_EDIT1`) and "Guild Duel Fee" (`WIDC_EDIT2`, which stays an inert edit in V15) with OK / Cancel. OK hands the trimmed name to the caller, which
// accepts a name of 3 or more and fewer than 48 characters (`SendDeclWar( idMaster, name )`); any other name does nothing and the window stays. Cancel closes. Centred.

import {
  APP_GUILD_WAR,
  WIDC_CANCEL,
  WIDC_EDIT1,
  WIDC_EDIT2,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { WarDeclRequest } from './ports';

export class WndGuildWarDecl extends WndNeuz {
  readonly appId = APP_GUILD_WAR;
  readonly #request: WarDeclRequest;

  constructor(request: WarDeclRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** The target guild name. */
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }
  /** "Guild Duel Fee": inert. */
  get feeEdit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT2);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        if (this.#request.submit((this.edit?.getString() ?? '').trim())) this.destroy();
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
