// `WndKawiBawiBoConfirm` (`CWndKawiBawiBoGameConfirm`, `src:_Interface/WndMiniGame.cpp:531-610`; WP-UI-63): the question of menu 44 on the layout of `APP_MESSAGEBOX` (256 x 144, `WndMessagebox.tga`). The
// original builds its controls in code like `CWndMessageBox`: a text control over the client deflated (10, 10, 10, 35) with `TID_GAME_KAWIBAWIBO_STARTGUID2` in black and an OK and a CANCEL
// button of 60 x 25 at `y = clientH - 30` (`ButtOk.tga`, `ButtCancel.tga`). OK asks the caller to open the game (the old game window is replaced), CANCEL closes; both close this window. It is
// not modal and Esc does not keep it. One at a time (`m_pWndKawiBawiBoGameConfirm == NULL`, the service replaces nothing: a second open is ignored by the feature).

import { APP_MESSAGEBOX } from '../../../gen/defs/resdata';
import { TID_GAME_KAWIBAWIBO_STARTGUID2 } from '../../../gen/defs/text';
import { Button } from '../../controls/Button';
import { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rectHeight, rectWidth } from '../../layout/geometry';
import { WBS_NODRAWFRAME, WBS_VSCROLL, Wnm } from '../../layout/styles';
import { MBID } from '../../msgbox/types';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { KawibawiboConfirmRequest, MiniGameHost } from './ports';

export class WndKawiBawiBoConfirm extends WndNeuz {
  readonly appId: number = APP_MESSAGEBOX;
  readonly #request: KawibawiboConfirmRequest;
  readonly #host: MiniGameHost;
  textCtrl: TextCtrl | null = null;
  okButton: Button | null = null;
  cancelButton: Button | null = null;

  constructor(request: KawibawiboConfirmRequest, host: MiniGameHost) {
    super();
    this.#request = request;
    this.#host = host;
    this.putRegInfo = false;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.rectClient;
    const cw = rectWidth(client);
    const ch = rectHeight(client);
    const x = Math.trunc(cw / 2);
    const y = ch - 30;
    const text = new TextCtrl();
    this.textCtrl = this.addControl(
      text,
      0,
      { l: 10, t: 10, r: cw - 10, b: ch - 35 },
      WBS_NODRAWFRAME | WBS_VSCROLL,
    );
    text.setString(this.#host.text(TID_GAME_KAWIBAWIBO_STARTGUID2), 0xff000000);
    const button = (id: number, left: number, caption: string, texture: string): Button => {
      const b = new Button();
      this.addControl(b, id, { l: left, t: y, r: left + 60, b: y + 25 });
      b.setTitle(caption);
      b.setTexture(texture);
      void b.fitTextureSize();
      return b;
    };
    this.okButton = button(MBID.OK, x - 60 - 10, 'OK', 'ButtOk.tga');
    this.cancelButton = button(MBID.CANCEL, x + 10, 'CANCEL', 'ButtCancel.tga');
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && (id === MBID.OK || id === MBID.CANCEL)) {
      if (id === MBID.OK) this.#request.ok();
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
