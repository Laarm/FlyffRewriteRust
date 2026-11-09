// The friend-summon windows (`CWndSummonFriend`, `CWndSummonFriendMsg`, `src:_Interface/WndSummonFriend.cpp:1-250`, Plan/42 §19.4 I, 21 §4.11; WP-UI-44).
//
//   `WndSummonFriend` (`APP_SUMMON_FRIEND`, 272 x 144): "Input the character name to be summoned." with the name field and OK / Cancel. The layout's combo box lists the online,
//     unblocked friends; the name can also be typed (a text field sits over its text area, a pick in the list fills it, as in the mail window). OK hands the text to the
//     request; a text of fewer than 3 or 64 and more bytes keeps the window (`request.submit` answers `false`), anything else closes it.
//   `WndSummonFriendMsg` (`APP_MESSAGEBOX_SUMMONFRINED`, 288 x 144): the friend's question "`<summoner>` summons you to `<world>`", OK confirms, Cancel declines; both close.

import {
  APP_MESSAGEBOX_SUMMONFRINED,
  APP_SUMMON_FRIEND,
  WIDC_CANCEL,
  WIDC_COMBOBOX1,
  WIDC_OK,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { ComboBox } from '../../controls/ComboBox';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { rect } from '../../layout/geometry';
import { WTYPE_EDITCTRL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { SummonFriendMsgRequest, SummonFriendRequest } from './ports';

/** The width of the drop button of the combo (the text field leaves it free). */
const DROP_BUTTON = 20;
const ID_NAME_FIELD = 0x7e71;

export class WndSummonFriend extends WndNeuz {
  readonly appId = APP_SUMMON_FRIEND;
  readonly #request: SummonFriendRequest;
  #combo: ComboBox | null = null;
  #field: EditCtrl | null = null;

  constructor(request: SummonFriendRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get combo(): ComboBox | null {
    return this.#combo;
  }
  /** The text field over the combo (what OK reads). */
  get field(): EditCtrl | null {
    return this.#field;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#combo = this.getDlgItem<ComboBox>(WIDC_COMBOBOX1);
    for (const f of this.#request.friends) this.#combo?.addString(f);
    const slot = this.getWndCtrl(WIDC_COMBOBOX1)?.rect;
    if (slot) {
      this.#field = this.createControl<EditCtrl>(
        WTYPE_EDITCTRL,
        ID_NAME_FIELD,
        rect(slot.l, slot.t, slot.r - DROP_BUTTON, slot.b),
      );
    }
    this.#field?.setFocus();
    this.moveParentCenter();
  }

  /** OK: `request.submit( text )`; the window closes unless the request keeps it. */
  ok(): void {
    if (this.#request.submit(this.#field?.getString() ?? '')) this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.ok();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    // a friend picked in the drop-down goes into the name field
    if (msg === Wnm.SELCHANGE && id === this.#combo?.id) {
      const name = this.#combo.getString();
      if (name !== '') this.#field?.setString(name);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}

export class WndSummonFriendMsg extends WndNeuz {
  readonly appId = APP_MESSAGEBOX_SUMMONFRINED;
  readonly #request: SummonFriendMsgRequest;
  #text: TextCtrl | null = null;
  #answered = false;

  constructor(request: SummonFriendMsgRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(WIDC_TEXT1);
    this.#text?.setString(this.#request.text);
    this.moveParentCenter();
  }

  ok(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.ok();
    this.destroy();
  }

  cancel(): void {
    if (this.#answered) return;
    this.#answered = true;
    this.#request.cancel();
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        this.ok();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.cancel();
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
