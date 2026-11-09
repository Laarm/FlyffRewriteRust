// `PetWindowsService` (`ui.pets`, Plan/42 §19.4 M / S; WP-UI-33, WP-UI-34): the pet windows.
//
//   ui.pets.openPetStatus(request)        APP_PET_STATUS: the summoned system pet (level, ability, life, experience, the stat strip)
//   ui.pets.openFoodConfirm(request)      APP_PET_ITEM: "how many?" for pet food and the food mill
//   ui.pets.openFoodMill(request)         APP_PET_FOODMILL: the feed grinder (NPC and feed-maker item)
//   ui.pets.openMiracle(request)          APP_PET_MIRACLE: the tamer's chance scroll
//   ui.pets.openTransEggs(request)        APP_PET_TRANS_EGGS: ten eggs into one
//   ui.pets.openPetRes(request)           APP_PET_RES: a B..S pet into the resurrection fragment
//   ui.pets.openBuffPetStatus(request)    APP_BUFFPET_STATUS: the sockets of an activated vis pet
//   ui.pets.setFilterPort(port)           the applet APP_PETFILTER (hotkey Y, `features.pet_filter`) opens with the saved mask and applies through this port
//
// Each `open*` replaces the window of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`). The requests carry the data and the
// callbacks; what the callbacks send is 45's (`features/pets`). `APP_PET_AWAK_CANCEL` is `ui.upgrade`'s, `APP_CHANGENAME` and `APP_CONFIRM_ENTER` are `ui.itemUse`'s.

import {
  APP_BUFFPET_STATUS,
  APP_PET_FOODMILL,
  APP_PET_ITEM,
  APP_PET_MIRACLE,
  APP_PET_RES,
  APP_PET_STATUS,
  APP_PET_TRANS_EGGS,
  APP_PETFILTER,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { PETS_LAZY } from './lazy';
import type {
  BuffPetRequest,
  FoodConfirmRequest,
  FoodMillRequest,
  PetFilterPort,
  PetMiracleRequest,
  PetResRequest,
  PetStatusRequest,
  TransEggsRequest,
} from './ports';
import type { WndBuffPetStatus } from './WndBuffPetStatus';
import type { WndFoodConfirm } from './WndFoodConfirm';
import type { WndPetFoodMill } from './WndPetFoodMill';
import type { WndPetMiracle } from './WndPetMiracle';
import type { WndPetRes } from './WndPetRes';
import type { WndPetStatus } from './WndPetStatus';
import type { WndPetTransEggs } from './WndPetTransEggs';
import { WndPetFilter } from './WndPetFilter';

export * from './ports';
export { PET_FILTER_ALL, PET_FILTER_BOXES } from './filterBits';
export type { WndBuffPetStatus } from './WndBuffPetStatus';
export type { WndFoodConfirm } from './WndFoodConfirm';
export type { WndPetFoodMill } from './WndPetFoodMill';
export type { WndPetMiracle } from './WndPetMiracle';
export type { WndPetRes } from './WndPetRes';
export type { WndPetStatus } from './WndPetStatus';
export type { WndPetTransEggs } from './WndPetTransEggs';
export { WndPetFilter } from './WndPetFilter';

/** The filter port before the feature installs the real one: nothing saved, nothing applied. */
export const NULL_FILTER: PetFilterPort = { current: () => 0, apply: () => undefined };

export interface PetWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
}

export class PetWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  #filter: PetFilterPort = NULL_FILTER;

  constructor(deps: PetWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    // the applet of the start menu and the hotkey Y opens with the installed port
    deps.wm.applets.register({
      app: APP_PETFILTER,
      factory: () => new WndPetFilter(this.#filter),
    });
  }

  /** Installs the port the filter applet reads and applies through. */
  setFilterPort(port: PetFilterPort | null): void {
    this.#filter = port ?? NULL_FILTER;
  }

  /** Closes the open window of an app so that a new one can take its place. */
  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  openPetStatus(request: PetStatusRequest): WndPetStatus | null {
    return this.#open(
      APP_PET_STATUS,
      PETS_LAZY.WndPetStatus,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openFoodConfirm(request: FoodConfirmRequest): WndFoodConfirm | null {
    return this.#open(APP_PET_ITEM, PETS_LAZY.WndFoodConfirm, (C) => new C(request));
  }

  openFoodMill(request: FoodMillRequest): WndPetFoodMill | null {
    return this.#open(
      APP_PET_FOODMILL,
      PETS_LAZY.WndPetFoodMill,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openMiracle(request: PetMiracleRequest): WndPetMiracle | null {
    return this.#open(APP_PET_MIRACLE, PETS_LAZY.WndPetMiracle, (C) => new C(request));
  }

  openTransEggs(request: TransEggsRequest): WndPetTransEggs | null {
    return this.#open(
      APP_PET_TRANS_EGGS,
      PETS_LAZY.WndPetTransEggs,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openPetRes(request: PetResRequest): WndPetRes | null {
    return this.#open(APP_PET_RES, PETS_LAZY.WndPetRes, (C) => new C(request, this.#tooltip));
  }

  openBuffPetStatus(request: BuffPetRequest): WndBuffPetStatus | null {
    return this.#open(APP_BUFFPET_STATUS, PETS_LAZY.WndBuffPetStatus, (C) => new C({ request }));
  }

  // ---- the open windows (the answers of the server and the feeds of the inventory go to them) ------------------------------------------

  petStatus(): WndPetStatus | null {
    return this.#wm.get(APP_PET_STATUS) as WndPetStatus | null;
  }
  foodConfirm(): WndFoodConfirm | null {
    return this.#wm.get(APP_PET_ITEM) as WndFoodConfirm | null;
  }
  foodMill(): WndPetFoodMill | null {
    return this.#wm.get(APP_PET_FOODMILL) as WndPetFoodMill | null;
  }
  miracle(): WndPetMiracle | null {
    return this.#wm.get(APP_PET_MIRACLE) as WndPetMiracle | null;
  }
  transEggs(): WndPetTransEggs | null {
    return this.#wm.get(APP_PET_TRANS_EGGS) as WndPetTransEggs | null;
  }
  petRes(): WndPetRes | null {
    return this.#wm.get(APP_PET_RES) as WndPetRes | null;
  }
  buffPetStatus(): WndBuffPetStatus | null {
    return this.#wm.get(APP_BUFFPET_STATUS) as WndBuffPetStatus | null;
  }
  petFilter(): WndPetFilter | null {
    return this.#wm.get(APP_PETFILTER) as WndPetFilter | null;
  }

  /** The pet ended (`OnPetRelease`): the status window, the chance window and the food count box go. */
  closeSystemPetWindows(): void {
    for (const app of [APP_PET_STATUS, APP_PET_MIRACLE, APP_PET_ITEM]) this.#wm.close(app);
  }
}
