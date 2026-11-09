// `WndRemoveAttribute` (`CWndRemoveAttribute`, `src:_Interface/WndField.cpp:22447-22650`, `__REMOVE_ATTRIBUTE`, ref/22 §17 L, Plan/45 §18.3;
// WP-UI-30 / WP-CFEAT-25): `APP_REMOVE_ATTRIBUTE` (272 x 256), opened by the NPC menu `MMI_ATTRIBUTE` ("Remove Element upgrading", 100 000 penya).
// One slot (`WIDC_PIC_SLOT`) takes a weapon or suit that carries an element (`IsEleRefineryAble && m_nResistAbilityOption > 0`, else the message
// box `TID_GAME_REMOVE_ERROR`); Start (`WIDC_START`, disabled until an item is there) asks `APP_REMOVE_ATTRIBUTE_CONFIRM`; Yes sends
// `RemoveAttributeReq{item}` and the window stays open until `RemoveAttributeNtf` arrives (`ReceiveResult` closes it, with or without success).
// The confirm window blocks a second Yes while the request is pending (the port's `pending()`).
// `WndRemoveAttributeConfirm` (`APP_REMOVE_ATTRIBUTE_CONFIRM`, 288 x 128): the text `TID_GAME_REMOVE_ATTRIBUTE`, Yes and No; No and the close
// button close the confirm only.

import {
  APP_REMOVE_ATTRIBUTE,
  APP_REMOVE_ATTRIBUTE_CONFIRM,
  WIDC_NO,
  WIDC_PIC_SLOT,
  WIDC_START,
  WIDC_TEXT1,
  WIDC_YES,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { DialogItem } from '../dialogs/ports';
import type { RemoveAttributeRequest } from './ports';
import { SlotWindow } from './SlotWindow';

export class WndRemoveAttribute extends SlotWindow<RemoveAttributeRequest> {
  readonly appId = APP_REMOVE_ATTRIBUTE;
  #confirm: WndRemoveAttributeConfirm | null = null;

  constructor(request: RemoveAttributeRequest, tooltip?: TooltipService) {
    super(request, {
      slotId: WIDC_PIC_SLOT,
      startId: WIDC_START,
      textId: WIDC_TEXT1,
      description: request.description,
      ...(tooltip ? { tooltip } : {}),
    });
  }

  /** The confirm window while it is open. */
  get confirmWindow(): WndRemoveAttributeConfirm | null {
    return this.#confirm && !this.#confirm.destroyed ? this.#confirm : null;
  }

  /** Start: `m_pWndConfirm == NULL` opens the confirm, a second press while it is open does nothing. */
  protected onStart(item: DialogItem): void {
    if (this.confirmWindow !== null) return;
    this.#confirm = this.env.wm.open(
      APP_REMOVE_ATTRIBUTE_CONFIRM,
      () =>
        new WndRemoveAttributeConfirm({
          text: this.request.confirmText,
          yes: () => {
            // the request answers with `RemoveAttributeNtf`; until then a second Yes sends nothing
            if (this.request.pending()) return;
            const it = this.item ?? item;
            this.request.send(it.objId);
          },
          closed: () => {
            this.#confirm = null;
          },
        }),
    );
  }

  /** `ReceiveResult`: the answer arrived (success or not), the window closes with its confirm. */
  receiveResult(): void {
    this.destroy();
  }

  /** The confirm goes with the window (in the same removal pass). */
  override destroy(): void {
    const confirm = this.#confirm;
    if (confirm && !confirm.destroyed) confirm.destroy();
    super.destroy();
  }

  override onDestroy(): void {
    const confirm = this.#confirm;
    this.#confirm = null;
    if (confirm && !confirm.destroyed) confirm.destroy();
    super.onDestroy();
  }
}

export interface RemoveAttributeConfirmOptions {
  readonly text: string;
  /** Yes: the window stays open. */
  yes(): void;
  closed(): void;
}

export class WndRemoveAttributeConfirm extends WndNeuz {
  readonly appId = APP_REMOVE_ATTRIBUTE_CONFIRM;
  readonly #opts: RemoveAttributeConfirmOptions;

  constructor(opts: RemoveAttributeConfirmOptions) {
    super();
    this.#opts = opts;
    this.putRegInfo = false;
  }

  get text(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get yesButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_YES);
  }
  get noButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_NO);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.text?.setString(this.#opts.text);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_YES) {
      this.#opts.yes();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_NO) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#opts.closed();
    super.onDestroy();
  }
}
