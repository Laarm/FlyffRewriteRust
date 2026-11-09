// `LogoutCountdownWnd` (`CWndLogOutWaitting`, WndField.cpp:12481-12556; Plan/42 §14.2): the countdown window of Logout and
// Quit on `APP_MESSAGE_TEXT` (240 x 80, `WndTile03`, `WBS_MOVE | WBS_SOUND | WBS_CAPTION | WBS_MODAL`). Its title bar is
// hidden (no close button: the countdown cannot be cancelled), it is moved to `((layout.r - w) / 2, 70)` and its text
// control `WIDC_TEXT1` shows "Quit after N sec." with the seconds left, refreshed every process tick. At 0 the port's
// `expired()` runs the leave and the window closes. While the window exists the window manager reports logout-pending:
// the key router swallows every game key (§5.6 rule 3) and the Esc chain and the taskbar slot hotkeys do nothing.

import { APP_MESSAGE_TEXT, WIDC_TEXT1 } from '../../gen/defs/resdata';
import type { TextCtrl } from '../controls/TextCtrl';
import { rectWidth } from '../layout/geometry';
import { WBS_CAPTION, WBS_MODAL, WBS_MOVE, WBS_SOUND } from '../layout/styles';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';
import type { LogoutCountdownView } from './ports';

/** The black of `CWndText::SetString(str)` without a colour. */
const TEXT_COLOR = 0xff000000;

export class LogoutCountdownWnd extends WndNeuz {
  readonly appId = APP_MESSAGE_TEXT;
  /** `m_bIsLogOut` (`SetIsLogOut`): informational; the port decides what the end does. */
  isLogOut = false;
  readonly #view: LogoutCountdownView;
  #shown = -1;
  #ended = false;

  constructor(view: LogoutCountdownView, isLogOut = false) {
    super();
    this.#view = view;
    this.isLogOut = isLogOut;
  }

  /** `Create(WBS_MOVE | WBS_SOUND | WBS_CAPTION | WBS_MODAL, rect of the layout, ..., APP_MESSAGE_TEXT)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MOVE | WBS_SOUND | WBS_CAPTION | WBS_MODAL | extraStyle);
  }

  /** The text control of the layout. */
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }

  /** The countdown window ticks every frame so that the seconds never lag behind the clock. */
  override readonly processEveryFrame = true;

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `m_wndTitleBar.SetVisible(FALSE)`: the window cannot be closed by the user
    this.titleBar?.setVisible(false);
    const layout = this.env.wm.layoutRect;
    this.move({ x: Math.trunc((layout.r - rectWidth(this.rectWindow)) / 2), y: 70 });
    this.#refresh();
  }

  override process(now: number): void {
    super.process(now);
    this.#refresh();
  }

  /** `Process`: the seconds left, the text while above 0, the end at 0. */
  #refresh(): void {
    if (this.#ended) return;
    const left = Math.max(0, Math.trunc(this.#view.secondsLeft()));
    if (left > 0) {
      if (left !== this.#shown) {
        this.#shown = left;
        this.textCtrl?.setString(this.#view.text(left), TEXT_COLOR);
      }
      return;
    }
    this.#ended = true;
    this.#view.expired?.();
    this.destroy();
  }

  override onDestroy(): void {
    if (!this.#ended) this.#view.closed?.();
    this.#ended = true;
    super.onDestroy();
  }
}
