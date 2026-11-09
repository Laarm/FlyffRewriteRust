// `UltimateWindowsService` (`ui.ultimate`, Plan/42 §19.4 L; WP-UI-32, WP-UI-33): the windows of the ultimate weapons and their jewels.
//
//   ui.ultimate.openMixJewel(request)       APP_SMELT_MIXJEWEL (+ APP_SMELT_MIXJEWEL_CONFIRM): five Orichalcum and five moonstones make a shining Orichalcum
//   ui.ultimate.openExtraction(request)     APP_SMELT_EXTRACTION: a general or unique weapon is destroyed for jewels
//   ui.ultimate.openSmeltJewel(request)     APP_SMELT_JEWEL: a jewel into the free socket of an ultimate weapon (the weapon turns first)
//   ui.ultimate.openChangeWeapon(request)   APP_SMELT_CHANGEWEAPON: a general weapon becomes unique, a unique one ultimate
//   ui.ultimate.openRemoveJewel(request)    APP_SMELT_REMOVE_JEWEL (+ APP_SMELT_REMOVE_CONFIRM): the last jewel of an ultimate weapon goes for a moonstone
//   ui.ultimate.mixJewel() ...              the open window of each kind, `null` when none (the `Ultimate` answers and the dispatch ask for them)
//
// Each `open*` replaces the window of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`). The requests carry the data
// and the callbacks; what the callbacks send is 45's (`features/ultimate`).

import {
  APP_SMELT_CHANGEWEAPON,
  APP_SMELT_EXTRACTION,
  APP_SMELT_JEWEL,
  APP_SMELT_MIXJEWEL,
  APP_SMELT_REMOVE_JEWEL,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { ULTIMATE_LAZY } from './lazy';
import type {
  ChangeWeaponRequest,
  ExtractionRequest,
  MixJewelRequest,
  RemoveJewelRequest,
  SmeltJewelRequest,
} from './ports';
import type { WndChangeWeapon } from './WndChangeWeapon';
import type { WndExtraction } from './WndExtraction';
import type { WndMixJewel } from './WndMixJewel';
import type { WndRemoveJewel } from './WndRemoveJewel';
import type { WndSmeltJewel } from './WndSmeltJewel';

export * from './ports';
export * from './results';
export * from './constants';
export { spinTimeline, SPIN_FPS, type SpinTimeline } from './spin';
// the shells load on demand (`lazy.ts`): only their types are exported here
export type { WndChangeWeapon, WndExtraction, WndMixJewel, WndRemoveJewel, WndSmeltJewel };
export type { WndMixJewelConfirm } from './WndMixJewel';
export type { WndRemoveJewelConfirm } from './WndRemoveJewel';
export type { WeaponStage } from './WeaponStage';

export interface UltimateWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class UltimateWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;

  constructor(deps: UltimateWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
  }

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  /** Opens the window of a lazy class (`null` while its chunk is still loading: the window then opens by itself). */
  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  openMixJewel(request: MixJewelRequest): WndMixJewel | null {
    return this.#open(
      APP_SMELT_MIXJEWEL,
      ULTIMATE_LAZY.WndMixJewel,
      (C) => new C(request, this.#getText),
    );
  }

  openExtraction(request: ExtractionRequest): WndExtraction | null {
    return this.#open(APP_SMELT_EXTRACTION, ULTIMATE_LAZY.WndExtraction, (C) => new C(request));
  }

  openSmeltJewel(request: SmeltJewelRequest): WndSmeltJewel | null {
    return this.#open(APP_SMELT_JEWEL, ULTIMATE_LAZY.WndSmeltJewel, (C) => new C(request));
  }

  openChangeWeapon(request: ChangeWeaponRequest): WndChangeWeapon | null {
    return this.#open(
      APP_SMELT_CHANGEWEAPON,
      ULTIMATE_LAZY.WndChangeWeapon,
      (C) => new C({ request, getText: this.#getText, tooltip: this.#tooltip }),
    );
  }

  openRemoveJewel(request: RemoveJewelRequest): WndRemoveJewel | null {
    return this.#open(
      APP_SMELT_REMOVE_JEWEL,
      ULTIMATE_LAZY.WndRemoveJewel,
      (C) => new C({ request, getText: this.#getText, tooltip: this.#tooltip }),
    );
  }

  mixJewel(): WndMixJewel | null {
    return this.#wm.get(APP_SMELT_MIXJEWEL) as WndMixJewel | null;
  }
  extraction(): WndExtraction | null {
    return this.#wm.get(APP_SMELT_EXTRACTION) as WndExtraction | null;
  }
  smeltJewel(): WndSmeltJewel | null {
    return this.#wm.get(APP_SMELT_JEWEL) as WndSmeltJewel | null;
  }
  changeWeapon(): WndChangeWeapon | null {
    return this.#wm.get(APP_SMELT_CHANGEWEAPON) as WndChangeWeapon | null;
  }
  removeJewel(): WndRemoveJewel | null {
    return this.#wm.get(APP_SMELT_REMOVE_JEWEL) as WndRemoveJewel | null;
  }
}
