// `UpgradeWindowsService` (`ui.upgrade`, Plan/42 §19.4 L; WP-UI-29, WP-UI-30, WP-UI-31): the upgrade, element, awakening and piercing windows.
//
//   ui.upgrade.openSmeltSafety(request)        APP_SMELT_SAFETY: the safe upgrade window (auto, or one try per Start; the state machine is the feature's)
//   ui.upgrade.openRemoveAttribute(request)    APP_REMOVE_ATTRIBUTE (+ APP_REMOVE_ATTRIBUTE_CONFIRM): take the element off a weapon or suit
//   ui.upgrade.openCommercialElem(request)     APP_COMMERCIAL_ELEM: apply or remove a commercial element stone
//   ui.upgrade.openAwakening(request)          APP_AWAKENING: awaken an item at the NPC
//   ui.upgrade.openSelectAwakeCase(request)    APP_AWAKE_SELECTCASE: the old / new option sets of a protected awakening (modal, 60 s)
//   ui.upgrade.openCancelBlessing(request)     APP_CANCEL_BLESSING: take the blessing off an item
//   ui.upgrade.openLvReqDown(request)          APP_LVREQDOWN: cancel the required-level reduction of an item
//   ui.upgrade.openPetAwakCancel(request)      APP_PET_AWAK_CANCEL: cancel the awakening of a pick-up pet
//   ui.upgrade.openPiercing(request)           APP_PIERCING: pierce an item (moonstone, optional protection scroll)
//   ui.upgrade.openRemovePiercing(request)     APP_SMELT_REMOVE_PIERCING(_EX): remove the last card of a pierced item
//   ui.upgrade.openUpgradeBase(request)        APP_TEST: the old supplement-stone upgrade (hidden in the port)
//
// Each `open*` replaces the window of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`). The requests carry the data and the
// callbacks; what the callbacks send is 45's (`features/upgrade`). `APP_SMELT_SAFETY_CONFIRM`, `APP_RANDOMSCROLL_CONFIRM`, `APP_RANDOMOPTIONRETRY`,
// `APP_CHANGE_ATTRIBUTE` and `APP_REMOVE_ELEM` belong to `ui.itemUse`.

import {
  APP_AWAKE_SELECTCASE,
  APP_AWAKENING,
  APP_CANCEL_BLESSING,
  APP_COMMERCIAL_ELEM,
  APP_LVREQDOWN,
  APP_PET_AWAK_CANCEL,
  APP_PIERCING,
  APP_REMOVE_ATTRIBUTE,
  APP_REMOVE_ATTRIBUTE_CONFIRM,
  APP_SMELT_REMOVE_PIERCING,
  APP_SMELT_REMOVE_PIERCING_EX,
  APP_SMELT_SAFETY,
  APP_TEST,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { UPGRADE_LAZY } from './lazy';
import type {
  CommercialElemRequest,
  PiercingRequest,
  RemoveAttributeRequest,
  RemovePiercingRequest,
  SelectAwakeRequest,
  SmeltSafetyRequest,
  StartItemRequest,
  UpgradeBaseRequest,
} from './ports';
import type { WndCommercialElem } from './WndCommercialElem';
import type { WndPiercing } from './WndPiercing';
import type { WndRemoveAttribute, WndRemoveAttributeConfirm } from './WndRemoveAttribute';
import type { WndRemovePiercing } from './WndRemovePiercing';
import type { WndSelectAwakeCase } from './WndSelectAwakeCase';
import type { WndSmeltSafety } from './WndSmeltSafety';
import type {
  WndAwakening,
  WndCancelBlessing,
  WndLvReqDown,
  WndPetAwakCancel,
} from './WndStartItem';
import type { WndUpgradeBase } from './WndUpgradeBase';

export * from './ports';
export { EnchantGlow, type GlowSpot } from './EnchantGlow';
export { KEEP_LABEL, SAFETY_FAILURE, SAFETY_GAUGE, SAFETY_SUCCESS, TAKE_LABEL } from './names';
export type { SlotWindow, SlotWindowOptions } from './SlotWindow';
export type {
  WndAwakening,
  WndCancelBlessing,
  WndCommercialElem,
  WndLvReqDown,
  WndPetAwakCancel,
  WndPiercing,
  WndRemoveAttribute,
  WndRemoveAttributeConfirm,
  WndRemovePiercing,
  WndSelectAwakeCase,
  WndSmeltSafety,
  WndUpgradeBase,
};

export interface UpgradeWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
}

export class UpgradeWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;

  constructor(deps: UpgradeWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
  }

  /** Closes the open window of an app so that a new one can take its place. */
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

  openSmeltSafety(request: SmeltSafetyRequest): WndSmeltSafety | null {
    return this.#open(
      APP_SMELT_SAFETY,
      UPGRADE_LAZY.WndSmeltSafety,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openRemoveAttribute(request: RemoveAttributeRequest): WndRemoveAttribute | null {
    return this.#open(
      APP_REMOVE_ATTRIBUTE,
      UPGRADE_LAZY.WndRemoveAttribute,
      (C) => new C(request, this.#tooltip),
    );
  }

  openCommercialElem(request: CommercialElemRequest): WndCommercialElem | null {
    return this.#open(
      APP_COMMERCIAL_ELEM,
      UPGRADE_LAZY.WndCommercialElem,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openAwakening(request: StartItemRequest): WndAwakening | null {
    return this.#open(
      APP_AWAKENING,
      UPGRADE_LAZY.WndAwakening,
      (C) => new C(request, this.#tooltip),
    );
  }

  openSelectAwakeCase(request: SelectAwakeRequest): WndSelectAwakeCase | null {
    return this.#open(APP_AWAKE_SELECTCASE, UPGRADE_LAZY.WndSelectAwakeCase, (C) => new C(request));
  }

  openCancelBlessing(request: StartItemRequest): WndCancelBlessing | null {
    return this.#open(
      APP_CANCEL_BLESSING,
      UPGRADE_LAZY.WndCancelBlessing,
      (C) => new C(request, this.#tooltip),
    );
  }

  openLvReqDown(request: StartItemRequest): WndLvReqDown | null {
    return this.#open(
      APP_LVREQDOWN,
      UPGRADE_LAZY.WndLvReqDown,
      (C) => new C(request, this.#tooltip),
    );
  }

  openPetAwakCancel(request: StartItemRequest): WndPetAwakCancel | null {
    return this.#open(
      APP_PET_AWAK_CANCEL,
      UPGRADE_LAZY.WndPetAwakCancel,
      (C) => new C(request, this.#tooltip),
    );
  }

  openPiercing(request: PiercingRequest): WndPiercing | null {
    return this.#open(
      APP_PIERCING,
      UPGRADE_LAZY.WndPiercing,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openRemovePiercing(request: RemovePiercingRequest): WndRemovePiercing | null {
    const app = request.extended ? APP_SMELT_REMOVE_PIERCING_EX : APP_SMELT_REMOVE_PIERCING;
    return this.#open(app, UPGRADE_LAZY.WndRemovePiercing, (C) => new C(request, this.#tooltip));
  }

  openUpgradeBase(request: UpgradeBaseRequest): WndUpgradeBase | null {
    return this.#open(
      APP_TEST,
      UPGRADE_LAZY.WndUpgradeBase,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  // ---- the open windows (the dispatch feeds them, the answers of the server close them) -----------------------------------------

  smeltSafety(): WndSmeltSafety | null {
    return this.#wm.get(APP_SMELT_SAFETY) as WndSmeltSafety | null;
  }
  removeAttribute(): WndRemoveAttribute | null {
    return this.#wm.get(APP_REMOVE_ATTRIBUTE) as WndRemoveAttribute | null;
  }
  removeAttributeConfirm(): WndRemoveAttributeConfirm | null {
    return this.#wm.get(APP_REMOVE_ATTRIBUTE_CONFIRM) as WndRemoveAttributeConfirm | null;
  }
  removePiercing(): WndRemovePiercing | null {
    return (this.#wm.get(APP_SMELT_REMOVE_PIERCING_EX) ??
      this.#wm.get(APP_SMELT_REMOVE_PIERCING)) as WndRemovePiercing | null;
  }
  selectAwakeCase(): WndSelectAwakeCase | null {
    return this.#wm.get(APP_AWAKE_SELECTCASE) as WndSelectAwakeCase | null;
  }
}
