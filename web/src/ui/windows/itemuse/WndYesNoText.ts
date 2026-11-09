// The "text and Yes / No" confirmations of the item flows (`CWndRandomScrollConfirm`, `CWndSmeltSafetyConfirm`,
// `src:_Interface/WndField.cpp:300-400, 25818-25900`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4; WP-UI-28; and WP-CFEAT-15 /
// WP-UI-30 / -34: `CWndRemoveElem`, `CWndRestateConfirm`, `CWndFunnyCoinConfirm`, `CWndConfirmVis`). All are layout windows with a
// read-only text and two buttons; they exist as windows (not as generic message boxes) because the dispatch refuses other item
// actions while they are open (`APP_SMELT_SAFETY_CONFIRM`: `TID_MMI_NOTUPGRADE`).
//
//   APP_RANDOMSCROLL_CONFIRM   the random-option scroll: "the target has / has no random option, continue?"; Yes sends
//   APP_SMELT_SAFETY_CONFIRM   no smelting protection scroll is active (three texts by mode); Yes continues the upgrade, No and
//                              the closing of the window put the cursor back to the base cursor
//   APP_REMOVE_ELEM            "the element stone on the worn item is lost, continue?": OK goes on with the equip / use, Cancel does nothing
//   APP_RESTATE_CONFIRM        the re-stat scroll: the text of the scroll, Yes uses it
//   APP_FUNNYCOIN_CONFIRM      "use <item>?": Yes uses the funny coin (the window closes on every click, as the original)
//   APP_CONFIRM_ENTER          the buff-pet / pick-up-pet / awake-protection question; modal, Enter = OK (`WBS_MODAL | WBS_KEY`)

import {
  APP_CONFIRM_ENTER,
  APP_FUNNYCOIN_CONFIRM,
  APP_RANDOMSCROLL_CONFIRM,
  APP_REMOVE_ELEM,
  APP_RESTATE_CONFIRM,
  APP_SMELT_SAFETY_CONFIRM,
  WIDC_BTN_ELEMCANCEL,
  WIDC_BTN_ELEMOK,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_CONFIRM_TEXT,
  WIDC_EDIT1,
  WIDC_NO,
  WIDC_RESTATE_CONFIRM_NO,
  WIDC_RESTATE_CONFIRM_TEXT,
  WIDC_RESTATE_CONFIRM_YES,
  WIDC_SMELT_NO,
  WIDC_SMELT_YES,
  WIDC_TEXT1,
  WIDC_TEXT_MESSAGE,
  WIDC_YES,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { WBS_KEY, WBS_MODAL, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { YesNoTextRequest } from './ports';

export abstract class WndYesNoText extends WndNeuz {
  protected abstract readonly textId: number;
  protected abstract readonly yesId: number;
  protected abstract readonly noId: number;
  /** `WBS_MODAL | WBS_KEY` with the Yes button as the default one (Enter answers Yes): `CWndConfirmVis`. */
  protected readonly modalEnter: boolean = false;
  /** Every notification of the window closes it (`CWndFunnyCoinConfirm::OnChildNotify` ends with `Destroy()`). */
  protected readonly closeOnAny: boolean = false;
  readonly #request: YesNoTextRequest;
  #text: TextCtrl | null = null;

  constructor(request: YesNoTextRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.#text;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, (this.modalEnter ? WBS_MODAL | WBS_KEY : 0) | extraStyle);
  }

  get yesButton(): Button | null {
    return this.getDlgItem<Button>(this.yesId);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(this.noId);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#text = this.getDlgItem<TextCtrl>(this.textId);
    this.#text?.setString(this.#request.text);
    this.#text?.enableWindow(false);
    if (this.modalEnter) this.yesButton?.setDefault(true);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === this.yesId) {
      this.#request.yes();
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === this.noId) {
      this.#request.no?.();
      this.destroy();
      return true;
    }
    if (this.closeOnAny) {
      const handled = super.onChildNotify(msg, id, src);
      this.destroy();
      return handled;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}

export class WndRandomScrollConfirm extends WndYesNoText {
  readonly appId = APP_RANDOMSCROLL_CONFIRM;
  protected readonly textId = WIDC_TEXT1;
  protected readonly yesId = WIDC_YES;
  protected readonly noId = WIDC_NO;
}

export class WndSmeltSafetyConfirm extends WndYesNoText {
  readonly appId = APP_SMELT_SAFETY_CONFIRM;
  protected readonly textId = WIDC_CONFIRM_TEXT;
  protected readonly yesId = WIDC_SMELT_YES;
  protected readonly noId = WIDC_SMELT_NO;
}

/** `CWndRemoveElem` (`APP_REMOVE_ELEM`, 272 x 160): the element stone of a worn weapon or suit is lost when the item is replaced. */
export class WndRemoveElem extends WndYesNoText {
  readonly appId = APP_REMOVE_ELEM;
  protected readonly textId = WIDC_TEXT_MESSAGE;
  protected readonly yesId = WIDC_BTN_ELEMOK;
  protected readonly noId = WIDC_BTN_ELEMCANCEL;
}

/** `CWndRestateConfirm` (`APP_RESTATE_CONFIRM`, 336 x 160): the re-stat scroll. */
export class WndRestateConfirm extends WndYesNoText {
  readonly appId = APP_RESTATE_CONFIRM;
  protected readonly textId = WIDC_RESTATE_CONFIRM_TEXT;
  protected readonly yesId = WIDC_RESTATE_CONFIRM_YES;
  protected readonly noId = WIDC_RESTATE_CONFIRM_NO;
}

/** `CWndFunnyCoinConfirm` (`APP_FUNNYCOIN_CONFIRM`, 288 x 128): every click (also on the text) ends the window. */
export class WndFunnyCoinConfirm extends WndYesNoText {
  readonly appId = APP_FUNNYCOIN_CONFIRM;
  protected readonly textId = WIDC_TEXT1;
  protected readonly yesId = WIDC_YES;
  protected readonly noId = WIDC_NO;
  protected override readonly closeOnAny = true;
}

/**
 * `CWndConfirmVis` (`APP_CONFIRM_ENTER`, 304 x 192): "equip the vis / put the key into the buff pet / turn the pick-up pet into a buff pet /
 * use the awakening protection?". Modal with the keyboard on the frame; OK (`WIDC_BUTTON1`) is the default button (the original reads Enter in
 * a hidden edit that it moves out of sight), Cancel (`WIDC_BUTTON2`) and the close button do nothing. The layout is shared with the campus
 * invitation (`CWndCampusInvitationConfirm`, M6).
 */
export class WndConfirmEnter extends WndYesNoText {
  readonly appId = APP_CONFIRM_ENTER;
  protected readonly textId = WIDC_TEXT1;
  protected readonly yesId = WIDC_BUTTON1;
  protected readonly noId = WIDC_BUTTON2;
  protected override readonly modalEnter = true;

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `pEdit->Move( -100, -100 ); pEdit->SetFocus();`: a modal window swallows Enter for the game (R6 of the key router), the focused hidden edit takes it and reports
    // `EN_RETURN`, which answers Yes like the default button
    const edit = this.getDlgItem<EditCtrl>(WIDC_EDIT1);
    edit?.move({ x: -100, y: -100 });
    edit?.setFocus();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_RETURN && id === WIDC_EDIT1) {
      return super.onChildNotify(Wnm.CLICKED, this.yesId, this.yesButton);
    }
    return super.onChildNotify(msg, id, src);
  }
}
