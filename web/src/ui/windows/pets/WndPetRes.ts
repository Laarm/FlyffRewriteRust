// `WndPetRes` (`CWndPetRes`, `src:_Interface/WndPetRes.cpp`, ref/22 §17 M, Plan/42 §19.4 M, 45 §18.7; WP-UI-34): `APP_PET_RES` (208 x 208, "Trade Window"), the pet
// exchange NPC that turns a B..S pet into the resurrection fragment. One slot (`WIDC_CHANGE`), the description (`WIDC_DESC`, `TID_GAME_PETTRADE`), OK and Cancel.
// A pet dropped from the bag fills the slot when the request accepts it (a pet of level B..S that is not the summoned one, not expired and usable; every refusal
// prints `TID_GAME_PETTRADE_ERROR` in red); a double click gives it back; OK sends `QuePetResurrection(item)` and ends the window, Cancel ends it; the pet is
// released when the window ends. The window opens at the right edge of the layout, 110 px down, and is then centred like every `Initialize`d dialog.

import {
  APP_PET_RES,
  WIDC_CANCEL,
  WIDC_CHANGE,
  WIDC_DESC,
  WIDC_OK,
} from '../../../gen/defs/resdata';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import type { DialogItem } from '../dialogs/ports';
import { SlotWindow } from '../upgrade/SlotWindow';
import type { PetResRequest } from './ports';

export class WndPetRes extends SlotWindow<PetResRequest> {
  readonly appId = APP_PET_RES;

  constructor(request: PetResRequest, tooltip?: TooltipService) {
    super(request, {
      slotId: WIDC_CHANGE,
      startId: WIDC_OK,
      textId: WIDC_DESC,
      description: request.description,
      ...(tooltip ? { tooltip } : {}),
    });
  }

  /** OK: the pet goes to the server (`m_pItemElem != NULL` is guaranteed by the enabled button). */
  protected onStart(item: DialogItem): void {
    this.startButton?.enableWindow(false);
    this.request.send(item.objId);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
