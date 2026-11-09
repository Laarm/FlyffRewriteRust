// `WndBeautyConfirm` (`APP_BEAUTY_SHOP_EX_CONFIRM`, 288 x 128, WBS_KEY, Plan/45 §17; WP-UI-35, WP-CFEAT-19): the confirmation of the
// beauty shops with Yes and No. Three texts use it, all built by the feature: the paid change (`CWndBeautyShopConfirm`: "Selected Change your
// Hair Price is 2,000,000 Penya. Do you want to change your hair?"), the coupon question of the item (`CWndUseCouponConfirm` flag 0, title
// `TID_GAME_USE_CHANGE_COUPON`) and the coupon change (flag 1: the question and `TID_GAME_WARNNING_USE_COUPON` in red on a line of its own).
// Yes runs the request (the feature sends or opens the coupon shop) and closes; No and the close button only close.

import {
  APP_BEAUTY_SHOP_EX_CONFIRM,
  WIDC_NO,
  WIDC_TEXT1,
  WIDC_YES,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_KEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { BEAUTY_WARNING_COLOR } from './beautyColors';
import type { BeautyConfirmRequest } from './ports';

export { BEAUTY_WARNING_COLOR };

export class WndBeautyConfirm extends WndNeuz {
  readonly appId = APP_BEAUTY_SHOP_EX_CONFIRM;
  readonly #request: BeautyConfirmRequest;
  #answered = false;

  constructor(request: BeautyConfirmRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | extraStyle);
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }
  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    if (this.#request.title !== undefined) this.setTitle(this.#request.title);
    this.text?.setString(this.#request.text);
    if (this.#request.warning !== undefined) {
      this.text?.addString('\n');
      this.text?.addString(this.#request.warning, BEAUTY_WARNING_COLOR);
    }
    this.yesButton?.setDefault(true);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_YES) {
      this.#answered = true;
      this.#request.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_NO) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    if (!this.#answered) {
      this.#answered = true;
      this.#request.no();
    }
    super.onDestroy();
  }
}
