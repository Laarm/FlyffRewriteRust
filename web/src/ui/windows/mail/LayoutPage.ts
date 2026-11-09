// `LayoutPage`: a tab page that is a LAYOUT window (`m_PostTabSend.Create(WBS_CHILD | WBS_NODRAWFRAME, rect, pWndTabCtrl, APP_POST_SEND)` of
// `CWndPost::OnInitialUpdate`): the window id is the applet id, so `CWndNeuz::OnInitialUpdate` builds the controls of that layout; the rect is the page
// rect of the tab (`(0, 0, w, h - 18)`), not the size of the layout (the layouts of the post tabs are taller than the page).

import { WBS_CHILD, WBS_NODRAWFRAME, WBS_NOFRAME } from '../../layout/styles';
import type { TabCtrl } from '../../controls/TabCtrl';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WndNeuz } from '../../wnd/WndNeuz';

export abstract class LayoutPage extends WndNeuz {
  /** Creates the page as a child of `tab` with the applet id as its window id. */
  attach(tab: TabCtrl, style = WBS_CHILD | WBS_NODRAWFRAME | WBS_NOFRAME): boolean {
    const w = rectWidth(tab.rectWindow);
    const h = rectHeight(tab.rectWindow) - 18;
    return this.create(style, { l: 0, t: 0, r: w, b: h }, tab, this.appId);
  }
}
