// `WndQueItmWarning` (`CWndQuestItemWarning`, `src:_Interface/WndField.cpp:405-480`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4,
// §9.6; WP-UI-22): `APP_QUEITMWARNING` (320 x 160), the yellow warning picture, a read-only text and Yes / No. It warns before
// a quest item is dropped (`TID_GAME_QUEITMWARNING`), a skill is reset or a post item is sent; Yes (or Enter) lets the caller
// go on (for a dropped quest item: the drop confirm or the quantity box), No closes.

import {
  APP_QUEITMWARNING,
  WIDC_BTN_NO,
  WIDC_BTN_YES,
  WIDC_CONTEXT,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { replaceWithText } from '../readOnlyText';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QueItemWarningRequest } from './ports';

export class WndQueItmWarning extends WndNeuz {
  readonly appId = APP_QUEITMWARNING;
  readonly #request: QueItemWarningRequest;
  #text: TextCtrl | null = null;

  constructor(request: QueItemWarningRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BTN_NO);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = replaceWithText(this, WIDC_CONTEXT);
    this.#text?.setString(this.#request.text);
    this.#text?.enableWindow(false);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if ((msg === Wnm.CLICKED && id === WIDC_BTN_YES) || msg === Wnm.EN_RETURN) {
      this.#request.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BTN_NO) {
      this.#request.no?.();
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
