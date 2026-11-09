// `CodePage`: a tab page that is built by code instead of from a layout (`CWndBase::Create(WBS_CHILD | ..., rect, tab, id)`
// of `CWndInventory::OnInitialUpdate` for the item grid, `CWndCharacter` for the character page, `CWndMotion` for the two
// icon pages). A page is a child frame of the tab control; the tab sizes it to `(0, 0, w, h - 18)`.

import { WBS_CHILD, WBS_NODRAWFRAME, WBS_NOFRAME } from '../../layout/styles';
import type { TabCtrl } from '../../controls/TabCtrl';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WndNeuz } from '../../wnd/WndNeuz';

export class CodePage extends WndNeuz {
  /** A page has no `ResData.h` id of its own. */
  readonly appId: number = 0;

  /** Creates the page as a child of `tab` with window id `id` (`style`: `WBS_CHILD | WBS_NOFRAME | WBS_NODRAWFRAME` by default). */
  attach(tab: TabCtrl, id: number, style = WBS_CHILD | WBS_NOFRAME | WBS_NODRAWFRAME): boolean {
    const w = rectWidth(tab.rectWindow);
    const h = rectHeight(tab.rectWindow) - 18;
    return this.create(style, { l: 0, t: 0, r: w, b: h }, tab, id);
  }
}
