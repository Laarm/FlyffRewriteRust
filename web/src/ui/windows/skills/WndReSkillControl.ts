// `WndReSkillControl` (`CWndReSkillControl`, `src:_Interface/WndField.cpp:14097-14160`, ref/22 §17 D, Plan/42 §19.4 D, 45 §19.1;
// WP-UI-38): `APP_RESKILL_CONTROL1` "Skill Distribution" (256 x 112), the old helper of the skill tree: a help text
// (`TID_RESKILLPOINT_CONTROL1`) and two buttons - Reset (`WIDC_BUTTON1`: the skill tree reloads the server state) and Finish
// (`WIDC_BUTTON2`: the re-skill warning). The title bar is hidden and the window cannot be closed with it (`nID == 10000` is
// swallowed). The V15 client never opens this window (the class has no creation site); the shell exists for the catalog.

import { APP_RESKILL_CONTROL1 } from '../../../gen/defs/resdata';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { ReSkillControlRequest } from './ports';

export class WndReSkillControl extends WndNeuz {
  readonly appId = APP_RESKILL_CONTROL1;
  readonly #request: ReSkillControlRequest;

  constructor(request: ReSkillControlRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>('WIDC_TEXT1');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#request.text);
    this.titleBar?.setVisible(false);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === this.getDlgItem('WIDC_BUTTON1')?.id) {
        this.#request.reset();
        return true;
      }
      if (id === this.getDlgItem('WIDC_BUTTON2')?.id) {
        this.#request.finish();
        return true;
      }
      // the original swallows the close button
      if (id === WTBID_CLOSE) return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
