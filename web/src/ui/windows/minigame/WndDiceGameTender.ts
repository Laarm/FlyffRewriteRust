// `WndDiceGameTender` (`CWndDiceGameTender`, `src:_Interface/WndMiniGame.cpp:1392-1510`; WP-UI-63): the stake box of the dice game on the layout of `APP_TRADE_GOLD` (116, 240 x 128; the real
// layout reads "Place your bet!" with a "Penya:" label). The edit starts with the stake now on the face. Editing clamps a value above the maximum to the maximum (`WIDC_EDIT` notification);
// OK and Enter read the number with `_ttoi` (a leading sign and digits, anything else stops it; nothing = 0): a value that is neither 0 nor at least the minimum opens the box
// `TID_GAME_DICETENDER_ERROR( min )` and the window stays (negative values too); anything else sends the stake (`SendFiveSystemBet( face, value )`, 0 withdraws it) and the window closes.
// Cancel closes. The window is a child of the dice window in the original (closed with it).

import { APP_TRADE_GOLD, WIDC_CANCEL, WIDC_EDIT, WIDC_OK } from '../../../gen/defs/resdata';
import { TID_GAME_DICETENDER_ERROR } from '../../../gen/defs/text';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DiceTenderRequest, MiniGameHost } from './ports';
import { ttoi } from './rows';

export class WndDiceGameTender extends WndNeuz {
  readonly appId = APP_TRADE_GOLD;
  readonly #request: DiceTenderRequest;
  readonly #host: MiniGameHost;
  #edit: EditCtrl | null = null;

  constructor(request: DiceTenderRequest, host: MiniGameHost) {
    super();
    this.#request = request;
    this.#host = host;
    this.putRegInfo = false;
  }

  get edit(): EditCtrl | null {
    return this.#edit;
  }
  get face(): number {
    return this.#request.face;
  }
  /** The number the edit holds now (`_ttoi`). */
  get value(): number {
    return ttoi(this.#edit?.getString() ?? '');
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#edit = this.getDlgItem<EditCtrl>(WIDC_EDIT);
    this.#edit?.setString(String(this.#request.preset));
    this.#edit?.setFocus();
    this.moveParentCenter();
  }

  /** The `WIDC_EDIT` notification: above the maximum the text becomes the maximum. */
  #clamp(): void {
    const edit = this.#edit;
    if (edit && ttoi(edit.getString()) > this.#request.max)
      edit.setString(String(this.#request.max));
  }

  override process(now: number): void {
    this.#clamp();
    super.process(now);
  }

  /** OK or Enter. */
  ok(): void {
    this.#clamp();
    const v = this.value;
    if (v < this.#request.min && v !== 0) {
      this.#host.box(this.#host.fmt(TID_GAME_DICETENDER_ERROR, this.#request.min));
      return;
    }
    this.#request.bet(v);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if ((msg === Wnm.CLICKED && id === WIDC_OK) || msg === Wnm.EN_RETURN) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
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
