// `WndPetFilter` (`CWndPetfilter`, `src:_Interface/WndField.cpp:26521-26640`, ref/22 §17 S, Plan/42 §19.4 S, 45 §18.7; WP-UI-34): `APP_PETFILTER` (272 x 256, hotkey Y with
// `features.pet_filter`), the loot filter of the eat pet: twelve check boxes (`WIDC_PF_*`), a SET bit means SKIP (Plan/19 §6.8). The boxes start from the saved mask;
// every click sets or clears its bit; the apply button (`WIDC_BUTTON1`) saves the mask, sends `PetFilter{mask}` and closes the window. (The original's `case WIDC_BUTTON1`
// falls through into the first box; it has no effect after `Destroy()`.) The window is moved to the centre of the screen when it opens.

import { APP_PETFILTER, WIDC_BUTTON1 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PET_FILTER_BOXES } from './filterBits';
import type { PetFilterPort } from './ports';

export class WndPetFilter extends WndNeuz {
  readonly appId = APP_PETFILTER;
  readonly #port: PetFilterPort;
  /** `dwPetfilter`: the working copy of the mask. */
  #mask = 0;

  constructor(port: PetFilterPort) {
    super();
    this.#port = port;
    this.putRegInfo = false;
  }

  /** The mask the boxes stand for. */
  get mask(): number {
    return this.#mask;
  }

  box(bit: number): Button | null {
    const b = PET_FILTER_BOXES.find((x) => x.bit === bit);
    return b ? this.getDlgItem<Button>(b.id) : null;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // `dwPetfilter = g_Option.m_dwPetfilter; SetButtonStatus()`: the saved mask (bits outside the twelve are kept as saved)
    this.#mask = this.#port.current() >>> 0;
    for (const b of PET_FILTER_BOXES) {
      this.getDlgItem<Button>(b.id)?.setCheck((this.#mask & b.bit) !== 0);
    }
    this.moveParentCenter();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_BUTTON1) {
        this.#port.apply(this.#mask >>> 0);
        this.destroy();
        return true;
      }
      const box = PET_FILTER_BOXES.find((b) => b.id === id);
      if (box) {
        const on = (this.getDlgItem<Button>(id)?.getCheck() ?? 0) !== 0;
        this.#mask = (on ? this.#mask | box.bit : this.#mask & ~box.bit) >>> 0;
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }
}
