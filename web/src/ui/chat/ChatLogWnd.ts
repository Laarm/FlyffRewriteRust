// `ChatLogWnd` (`CWndChatLog`, WndCommunication.cpp:1317-1533; Plan/42 §13.4): the system log window on `APP_CHAT_LOG`
// (240 x 128, `WndTile308`, resizable, minimum 272 x 48). It is created hidden together with the chat window and shown
// by the chat window's `WIDC_CHECK3`. The router writes the lines no chat tab shows into it; a right click opens a
// one-item menu ("Hide") that hides the window again and clears the toggle.

import {
  APP_CHAT_LOG,
  APP_COMMUNICATION_CHAT,
  WIDC_CUSTOM1,
  WIDC_TEXT,
} from '../../gen/defs/resdata';
import { TID_GAME_GUIDE_HIDE } from '../../gen/defs/text';
import { Button } from '../controls/Button';
import { Menu } from '../controls/Menu';
import { TextCtrl } from '../controls/TextCtrl';
import {
  type FrameRects,
  type Point,
  type Rect,
  rectDeflate,
  rectHeight,
} from '../layout/geometry';
import { WBS_NODRAWFRAME, WBS_NOFRAME, WBS_THICKFRAME, WBS_VSCROLL } from '../layout/styles';
import { adjustMinRect } from '../wm/dragMath';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import type { Shortcut } from '../dnd/shortcut';
import { appendChatLine } from './appendLine';
import { ChatWnd } from './ChatWnd';
import { LOG_MIN_H, LOG_MIN_W } from './geometry';
import type { ChatHost } from './host';
import type { ChatTextTarget } from './ports';

export class ChatLogWnd extends WndNeuz implements ChatTextTarget {
  readonly appId = APP_CHAT_LOG;
  text!: TextCtrl;
  /** `m_wndMenuPlace`: the right-click menu. */
  menu: Menu | null = null;
  readonly #host: ChatHost;

  constructor(host: ChatHost) {
    super();
    this.#host = host;
  }

  /** `Initialize`: `InitDialog(APP_CHAT_LOG, WBS_THICKFRAME)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_THICKFRAME | extraStyle);
  }

  /** `CWndChatLog::SetWndRect`: minimum 272 x 48, client = window deflated `(4, 4, 6, 6)`, layout = client. */
  protected override computeRects(window: Rect): FrameRects {
    const w = adjustMinRect(window, this.rectWindowValue, LOG_MIN_W, LOG_MIN_H);
    const client = rectDeflate(w, 4, 4, 6, 6);
    return { window: w, client, layout: { ...client } };
  }

  override constrainResize(next: Rect, current: Rect): Rect {
    return adjustMinRect(next, current, LOG_MIN_W, LOG_MIN_H);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const textRect = this.getWndCtrl(WIDC_CUSTOM1)?.rect ?? { l: 8, t: 10, r: 220, b: 110 };
    this.text = this.addControl(
      new TextCtrl(),
      WIDC_TEXT,
      textRect,
      WBS_NOFRAME | WBS_NODRAWFRAME | WBS_VSCROLL,
    );
    this.text.setEnableClipboard(this.#host.ports().view.isGm());
    this.text.setLineSpace(2);
    // bottom left of the layout rect
    const layout = this.env.wm.layoutRect;
    this.move({ x: 0, y: layout.b - rectHeight(this.rectWindowValue) });
    const menu = new Menu();
    menu.createMenu(this);
    menu.appendMenu(this.#host.text(TID_GAME_GUIDE_HIDE) || 'Hide', 0);
    this.menu = menu;
  }

  override onSize(type: number, cx: number, cy: number): void {
    const c = this.getClientRect(false);
    this.text.setWndRect({ l: 0, t: 0, r: c.r, b: c.b }, true);
    this.text.scrollToEnd();
    super.onSize(type, cx, cy);
  }

  /** `CWndChatLog::PutString`: the same append rule as the chat window, no fade. */
  put(text: string, color: number): void {
    appendChatLine(this.text, text, color);
  }

  // ---- pointer -------------------------------------------------------------------------------------------------

  override onLButtonDown(pt?: Point): void {
    if (pt) this.forwardToWorld('lbuttondown', pt);
  }

  /** `OnRButtonDown`: toggles the menu at the pointer. */
  override onRButtonDown(pt?: Point): void {
    const menu = this.menu;
    if (!menu || !pt) return;
    const origin = this.getScreenClientRect();
    menu.move({ x: origin.l + pt.x, y: origin.t + pt.y });
    menu.setVisible(!menu.isVisible());
    menu.setFocus();
  }

  forwardToWorld(
    kind: 'lbuttondown' | 'lbuttonup' | 'rbuttondown' | 'rbuttonup' | 'mousemove',
    pt: Point,
  ): void {
    const view = this.#host.ports().view;
    const o = this.getScreenClientRect();
    view.worldPointer?.(kind, { x: pt.x + o.l, y: pt.y + o.t });
  }

  /** The menu item: hides the window and clears the chat window's toggle. */
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (src instanceof Button && src.parent === this.menu) {
      this.setFocus();
      if (id === 0) {
        const chat = this.env.wm.get(APP_COMMUNICATION_CHAT);
        if (chat instanceof ChatWnd) chat.systemLogHidden();
        this.setVisible(false);
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
