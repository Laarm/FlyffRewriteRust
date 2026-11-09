// The three party-summon windows (`CWndSummonParty`, `CWndSummonPartyUse`, `CWndSummonPartyMsg`, `src:_Interface/WndSummonFriend.cpp:226-494`,
// ref/22 §17 H, Plan/42 §19.4 H, 45 §22.1; WP-UI-40, P2):
//
//   * `APP_SUMMON_PARTY` (288 x 224): the leader used the scroll "Summon Party"; the text asks whether to summon the members, OK sends
//     `SummonParty`, Cancel closes;
//   * `APP_MESSAGEBOX_SUMMONPARTY` (320 x 224): the notice a member gets with `Summon{party confirm}`: the text ("<party> has asked to summon
//     you ..."), how to use the scroll icon, the scroll's picture at (12, 116); OK closes;
//   * `APP_SUMMON_PARTY_USE` (288 x 144): the member double-clicked the scroll's buff icon; OK sends `SummonPartyConfirm`, Cancel closes.
//
// All three are centred.

import {
  APP_MESSAGEBOX_SUMMONPARTY,
  APP_SUMMON_PARTY,
  APP_SUMMON_PARTY_USE,
  WIDC_CANCEL,
  WIDC_OK,
  WIDC_TEXT1,
  WIDC_TEXT2,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { SpriteLayer } from '../../controls/SpriteLayer';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { SummonPartyMsgRequest, SummonPartyRequest, SummonPartyUseRequest } from './ports';

/** The scroll's picture in the member's notice (`CPoint( sx + 4, sy - 4 )` with `sx = 8, sy = 120`). */
export const SUMMON_ICON_POS = { x: 12, y: 116 } as const;

export class WndSummonParty extends WndNeuz {
  readonly appId = APP_SUMMON_PARTY;
  readonly #request: SummonPartyRequest;

  constructor(request: SummonPartyRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#request.text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.#request.ok();
        this.destroy();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}

export class WndSummonPartyUse extends WndNeuz {
  readonly appId = APP_SUMMON_PARTY_USE;
  readonly #request: SummonPartyUseRequest;

  constructor(request: SummonPartyUseRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#request.text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.#request.ok();
        this.destroy();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}

export class WndSummonPartyMsg extends WndNeuz {
  readonly appId = APP_MESSAGEBOX_SUMMONPARTY;
  readonly #request: SummonPartyMsgRequest;
  #icon: SpriteLayer | null = null;

  constructor(request: SummonPartyMsgRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get how(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT2);
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  /** The layer that draws the scroll's picture. */
  get icon(): SpriteLayer | null {
    return this.#icon;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#request.text);
    this.how?.setString(this.#request.how);
    this.#icon = this.addControl(new SpriteLayer(), 0x7e10, this.getClientRect(false));
    this.#icon.setEntries(
      this.#request.icon === null
        ? []
        : [
            {
              key: 'scroll',
              path: this.#request.icon,
              x: SUMMON_ICON_POS.x,
              y: SUMMON_ICON_POS.y,
              w: 32,
              h: 32,
            },
          ],
    );
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
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
