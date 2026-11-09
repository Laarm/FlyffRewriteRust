// `WndSelectVillage` (`CWndSelectVillage`, `src:_Interface/WndSelectVillage.cpp`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4; WP-UI-28):
// `APP_COMM_SELECTVIL` (176 x 144), the destination of a return scroll: three radios (Flaris, Saint Morning, Darkon; the first one
// checked and the group's start) and OK / Cancel. OK sends the index of the checked radio (`SendReturnScroll(i)`) and closes.

import {
  APP_COMM_SELECTVIL,
  WIDC_BUTTON_CANCEL,
  WIDC_BUTTON_OK,
  WIDC_RADIO_DARKON,
  WIDC_RADIO_FLARIS,
  WIDC_RADIO_SAINTM,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { SelectVillageRequest } from './ports';

const RADIOS = [WIDC_RADIO_FLARIS, WIDC_RADIO_SAINTM, WIDC_RADIO_DARKON] as const;

export class WndSelectVillage extends WndNeuz {
  readonly appId = APP_COMM_SELECTVIL;
  readonly #request: SelectVillageRequest;

  constructor(request: SelectVillageRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  /** The radio buttons in the order of the villages. */
  get radios(): readonly (Button | null)[] {
    return RADIOS.map((id) => this.getDlgItem<Button>(id));
  }

  /** The index of the checked radio (`i` of `OnChildNotify`; 3 when none is checked). */
  get selected(): number {
    const i = this.radios.findIndex((b) => (b?.getCheck() ?? 0) !== 0);
    return i < 0 ? RADIOS.length : i;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const first = this.getDlgItem<Button>(RADIOS[0]);
    first?.setGroup(true);
    first?.setCheck(true);
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON_OK) {
      this.#request.select(this.selected);
      this.destroy();
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON_CANCEL) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
