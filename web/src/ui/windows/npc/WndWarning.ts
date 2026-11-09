// `WndWarning` (`CWndWarning`, `src:_Interface/WndShop.cpp:30-70`, ref/22 §17 E, Plan/45 §12.3; WP-UI-24): `APP_WARNING`
// (320 x 144, `WBS_KEY`), "this is a premium item, are you sure?": selling a charged item (or a high-level unique) goes through
// it first. OK continues to the sell confirmation (the request), Cancel closes.

import { APP_WARNING, WIDC_CANCEL, WIDC_OK } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_KEY, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { WarningRequest } from './ports';

export class WndWarning extends WndNeuz {
  readonly appId = APP_WARNING;
  readonly #request: WarningRequest;

  constructor(request: WarningRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** `InitDialog(.., WBS_KEY, ..)` + `MoveParentCenter()`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | extraStyle);
  }

  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.getDlgItem<Button>(WIDC_OK)?.setDefault(true);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.#request.ok();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
