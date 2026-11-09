// The four "item, description, Start" windows (Plan/42 §19.4 L, 45 §18.4; WP-UI-31 / WP-CFEAT-25), all `SlotWindow`s: Start sends the request with the
// item and closes the window (`Destroy()` right after the send; the destructor releases the reservation).
//   * `WndAwakening`     (`CWndAwakening`, `src:_Interface/WndAwakening.cpp`, `APP_AWAKENING` 272 x 208, `MMI_ITEM_AWAKENING`): an item that awakens and has no
//                        lines yet; the slot is `WIDC_STATIC1`, Start `WIDC_BUTTON1`, the text `WIDC_TEXT1` (`ItemAwakening.inc`);
//   * `WndCancelBlessing` (`CWndBlessingCancel`, `WndBlessingCancel.cpp`, `APP_CANCEL_BLESSING`, `MMI_BLESSING_CANCEL`): a blessed item with lines; slot
//                        `WIDC_CHANGE`, Start `WIDC_START`, text `WIDC_DESC` (`ItemBlessingCancel.inc`);
//   * `WndLvReqDown`     (`CWndLvReqDown`, `WndLvReqDown.cpp`, `APP_LVREQDOWN`, `MMI_LVREQDOWN_CANCEL`): an item whose required level was lowered (`GetLevelDown() < 0`);
//                        slot `WIDC_STATIC1`, Start `WIDC_BUTTON1`, text `WIDC_TEXT1` (`ReqLvDown.inc`);
//   * `WndPetAwakCancel` (`CWndPetAwakCancel`, `WndPetSys.cpp:15-160`, `APP_PET_AWAK_CANCEL`, `MMI_PET_AWAK_CANCEL`): a pick-up pet (the server checks the awakening);
//                        slot `WIDC_STATIC1`, Start `WIDC_BUTTON1`, text `WIDC_TEXT1` (`PetAwakCancel.inc`).
// What an item must be to enter a slot is the request's `accept`, which prints the refusal of the original.

import {
  APP_AWAKENING,
  APP_CANCEL_BLESSING,
  APP_LVREQDOWN,
  APP_PET_AWAK_CANCEL,
  WIDC_BUTTON1,
  WIDC_CHANGE,
  WIDC_DESC,
  WIDC_START,
  WIDC_STATIC1,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { DialogItem } from '../dialogs/ports';
import type { StartItemRequest } from './ports';
import { SlotWindow, type SlotWindowOptions } from './SlotWindow';

abstract class StartItemWindow extends SlotWindow<StartItemRequest> {
  #sent = false;

  protected constructor(
    request: StartItemRequest,
    opts: Omit<SlotWindowOptions, 'description'>,
    tooltip?: TooltipService,
  ) {
    super(request, {
      ...opts,
      description: request.description,
      ...(tooltip ? { tooltip } : {}),
    });
  }

  /** Start: the request is sent and the window ends (the button is disabled meanwhile, so a double click sends once). */
  protected onStart(item: DialogItem): void {
    if (this.#sent) return;
    this.#sent = true;
    this.startButton?.enableWindow(false);
    this.request.send(item.objId);
    this.destroy();
  }
}

export class WndAwakening extends StartItemWindow {
  readonly appId = APP_AWAKENING;
  constructor(request: StartItemRequest, tooltip?: TooltipService) {
    super(request, { slotId: WIDC_STATIC1, startId: WIDC_BUTTON1, textId: WIDC_TEXT1 }, tooltip);
  }
}

export class WndCancelBlessing extends StartItemWindow {
  readonly appId = APP_CANCEL_BLESSING;
  constructor(request: StartItemRequest, tooltip?: TooltipService) {
    super(request, { slotId: WIDC_CHANGE, startId: WIDC_START, textId: WIDC_DESC }, tooltip);
  }
}

export class WndLvReqDown extends StartItemWindow {
  readonly appId = APP_LVREQDOWN;
  constructor(request: StartItemRequest, tooltip?: TooltipService) {
    super(request, { slotId: WIDC_STATIC1, startId: WIDC_BUTTON1, textId: WIDC_TEXT1 }, tooltip);
  }
}

export class WndPetAwakCancel extends StartItemWindow {
  readonly appId = APP_PET_AWAK_CANCEL;
  constructor(request: StartItemRequest, tooltip?: TooltipService) {
    super(request, { slotId: WIDC_STATIC1, startId: WIDC_BUTTON1, textId: WIDC_TEXT1 }, tooltip);
  }
}
