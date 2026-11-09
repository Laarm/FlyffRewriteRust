// `WndCloseExistingConnection` (`APP_CLOSE_EXISTING_CONNECTION`, 12 §14, 45 §3.2 step 5, ref/22 §17 A): the yes/no box
// "This account is already connected. Would you like to cut the existing connection?" (`TID_DIAG_0065`). The layout is a
// `WndMessagebox` frame with two textured buttons (`WIDC_YES`, `WIDC_NO`) and NO text control (the guild-war confirm
// windows that share the layout draw their own text), so the text is a `TextCtrl` over the area above the buttons.

import { APP_CLOSE_EXISTING_CONNECTION, WIDC_NO, WIDC_YES } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { TextCtrl } from '../../controls/TextCtrl';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_KEY, WBS_MODAL, WBS_NODRAWFRAME, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { TakeoverActions, TakeoverView, ViewSource } from './ports';

export class WndCloseExistingConnection extends WndNeuz {
  readonly appId = APP_CLOSE_EXISTING_CONNECTION;
  readonly #model: ViewSource<TakeoverView>;
  readonly #actions: TakeoverActions;
  #text: TextCtrl | null = null;
  #off: (() => void) | null = null;

  constructor(model: ViewSource<TakeoverView>, actions: TakeoverActions) {
    super();
    this.#model = model;
    this.#actions = actions;
    this.putRegInfo = false;
  }

  /** Modal; Enter = Yes (the first button is the default one, like every standard box). */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | WBS_KEY | extraStyle);
  }

  get textCtrl(): TextCtrl | null {
    return this.#text;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.rectClient;
    const cw = rectWidth(client);
    const ch = rectHeight(client);
    const yes = this.getDlgItem<Button>(WIDC_YES);
    const no = this.getDlgItem<Button>(WIDC_NO);
    // the layout captions are the placeholder "Button": the sprites carry the words
    yes?.setTitle('');
    no?.setTitle('');
    yes?.setDefault(true);
    const text = new TextCtrl();
    this.#text = this.addControl(
      text,
      0,
      { l: 10, t: 10, r: cw - 10, b: Math.min(80, ch - 35) },
      WBS_NODRAWFRAME,
    );
    text.setString(this.#model.get().text, 0xff000000);
    this.moveParentCenter();
    this.#off = this.#model.subscribe(() => {
      this.#text?.setString(this.#model.get().text, 0xff000000);
    });
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_YES) {
        this.destroy();
        this.#actions.yes();
        return true;
      }
      if (id === WIDC_NO) {
        this.destroy();
        this.#actions.no();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    super.onDestroy();
  }
}
