// The common shell of the three Yes / No questions of the guild war (`CWndGuildWarGiveUp`, `CWndGuildWarPeace`, `CWndGuildWarPeaceConfirm`, `src:_Interface/WndGuildWar*.cpp`;
// WP-UI-48): a layout window with a fixed question and Yes (`WIDC_YES`) / No (`WIDC_NO`), centred. Yes asks the caller to send and closes, No and the close button close.

import { WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { WarYesNoRequest } from './ports';

export abstract class WndWarYesNo extends WndNeuz {
  readonly #request: WarYesNoRequest;

  constructor(request: WarYesNoRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
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
