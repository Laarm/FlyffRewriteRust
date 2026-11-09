// `WndDiceGameMessage` (`CWndDiceGameMessage`, `src:_Interface/WndMiniGame.cpp:1512-1596`; WP-UI-63): the result box of the dice game on the layout of `APP_MESSAGEBOX`: the text in black over the
// client deflated (10, 10, 10, 35) and one OK button (60 x 25, `ButtOk.tga`, centred at `y = clientH - 30`). It stands at the bottom-right corner of the dice window minus (290, 150) (screen
// coordinates). OK resets the stakes and re-enables the dice window's buttons (the request's `ok`) and closes.

import { APP_MESSAGEBOX, APP_MINIGAME_DICE } from '../../../gen/defs/resdata';
import { Button } from '../../controls/Button';
import { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NODRAWFRAME, WBS_VSCROLL, Wnm } from '../../layout/styles';
import { MBID } from '../../msgbox/types';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DiceMessageRequest } from './ports';

export class WndDiceGameMessage extends WndNeuz {
  readonly appId: number = APP_MESSAGEBOX;
  readonly #request: DiceMessageRequest;
  textCtrl: TextCtrl | null = null;
  okButton: Button | null = null;

  constructor(request: DiceMessageRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.rectClient;
    const cw = rectWidth(client);
    const ch = rectHeight(client);
    const text = new TextCtrl();
    this.textCtrl = this.addControl(
      text,
      0,
      { l: 10, t: 10, r: cw - 10, b: ch - 35 },
      WBS_NODRAWFRAME | WBS_VSCROLL,
    );
    text.setString(this.#request.text, 0xff000000);
    const x = Math.trunc(cw / 2);
    const left = x - 30;
    const ok = new Button();
    this.okButton = this.addControl(ok, MBID.OK, { l: left, t: ch - 30, r: left + 60, b: ch - 5 });
    ok.setTitle('OK');
    ok.setTexture('ButtOk.tga');
    void ok.fitTextureSize();
    // the box sits at the bottom-right of the dice window minus (290, 150)
    const dice = this.env.wm.get(APP_MINIGAME_DICE);
    if (dice) {
      const r = dice.getScreenRect();
      this.move({ x: r.r - 290, y: r.b - 150 });
    }
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === MBID.OK) {
      this.#request.ok();
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
