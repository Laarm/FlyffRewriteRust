// `ChatFilterWnd` (`CWndChatFilter`, WndChatFilter.cpp; Plan/42 §13.4): the modal `APP_CHAT_FILTER` window of the chat
// window. In the shipped version (`__Y_CHAT_SYSTEM_8`) it shows only the five channel check boxes: they edit
// `chatFilter[tab]` of the CURRENT chat tab (the check of the tab's own channel is disabled: a tab always shows its
// channel). The lower part of the layout (game, notice and help message groups) is cut off by shrinking the window by
// 130 px and hiding the second group box. Every change is written through `ChatFilterActions` and the chat window
// refills its text area.

import {
  APP_CHAT_FILTER,
  WIDC_CHECK1,
  WIDC_CHECK2,
  WIDC_CHECK3,
  WIDC_CHECK4,
  WIDC_CHECK5,
  WIDC_GROUPBOX2,
} from '../../gen/defs/resdata';
import type { Button } from '../controls/Button';
import { rectHeight } from '../layout/geometry';
import { WBS_MODAL, Wnm } from '../layout/styles';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import type { ChatHost } from './host';
import { CHANNELS } from './styles';

const CHECKS = [WIDC_CHECK1, WIDC_CHECK2, WIDC_CHECK3, WIDC_CHECK4, WIDC_CHECK5] as const;

/** The window shrinks by this much (`rect.bottom - 130`). */
export const FILTER_CUT = 130;

/** The owner the filter reports to (the chat window). */
export interface ChatFilterOwner {
  setChannel(): void;
  filterClosed(w: ChatFilterWnd): void;
}

export class ChatFilterWnd extends WndNeuz {
  readonly appId = APP_CHAT_FILTER;
  readonly #host: ChatHost;
  readonly #owner: ChatFilterOwner;

  constructor(host: ChatHost, owner: ChatFilterOwner) {
    super();
    this.#host = host;
    this.#owner = owner;
  }

  /** `InitDialog(APP_CHAT_FILTER, WBS_MODAL, ...)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#buttonStatus();
    // the shipped window keeps only the channel part
    const client = this.getClientRect(false);
    this.setWndRect({ l: 0, t: 0, r: client.r, b: client.b - FILTER_CUT }, true);
    this.getDlgItem(WIDC_GROUPBOX2)?.setVisible(false);
    // controls that do not fit any more are clipped and unreachable in the original: hide them
    const bottom = rectHeight(this.rectClientValue);
    for (const c of this.controls) {
      if (c.rectWindow.b > bottom) c.setVisible(false);
    }
    this.moveParentCenter();
  }

  /** `SetButtonStatus`: the checks show `chatFilter[tab]`, the tab's own channel cannot be switched off. */
  #buttonStatus(): void {
    const router = this.#host.router;
    const tab = router.currentTab;
    const mask = router.masks().filters[tab] ?? 0;
    CHECKS.forEach((id, i) => {
      const b = this.getDlgItem<Button>(id);
      if (!b) return;
      const bit = CHANNELS[i] ?? 0;
      b.setCheck((mask & bit) !== 0);
      b.enableWindow(i !== tab);
    });
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const i = CHECKS.indexOf(id as (typeof CHECKS)[number]);
    if (i >= 0 && msg === Wnm.CLICKED) {
      const router = this.#host.router;
      const tab = router.currentTab;
      const { filters, etc } = router.masks();
      const on = (this.getDlgItem<Button>(id)?.getCheck() ?? 0) !== 0;
      const bit = CHANNELS[i] ?? 0;
      const next = filters.map((f, t) => (t === tab ? (on ? f | bit : f & ~bit) >>> 0 : f));
      this.#host.ports().filter.setFilters(next, etc);
      this.#buttonStatus();
    }
    // every notification refills the chat text (`pWndChat->SetChannel()` at the end of `OnChildNotify`)
    if (msg === Wnm.CLICKED) this.#owner.setChannel();
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#owner.filterClosed(this);
    super.onDestroy();
  }
}
