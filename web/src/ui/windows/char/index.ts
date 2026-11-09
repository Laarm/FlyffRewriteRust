// `CharService` (`ui.char`, Plan/42 §19.4 D; WP-UI-21): the character core windows - inventory, status, character (with the
// stat-point confirm) and motion - and their ports.
//
//   ui.char.setPorts({inventory, status, character, motion, previews})   45 WP-CFEAT-09/10/11/51: views and actions
//   ui.char.openStateConfirm(request)                                       APP_STATE_CONFIRM (Apply of the character page)
//   ui.char.statConfirm                                                     {yes, cancel} for 43's key router (`KillWindow`)
//
// The service registers the four applets (`ui.applets`) so that the taskbar, the start menu and the hotkeys open them; the
// window factories read the ports at the moment a window is created, so ports installed later reach windows opened later.

import {
  APP_CHARACTER3,
  APP_INVENTORY,
  APP_MOTION,
  APP_STATE_CONFIRM,
  APP_STATUS1,
} from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { openLazy } from '../../wnd/lazy';
import { honorDeps } from '../couple/HonorPage';
import { constantView as constantHonor, EMPTY_HONOR_VIEW, type HonorPorts } from '../couple/ports';
import { CHAR_LAZY } from './lazy';
import {
  constantView,
  type CharacterView,
  type CoreWindowPorts,
  type InventoryView,
  type MotionView,
  type StateConfirmRequest,
  type StatusView,
} from './ports';
import { WndCharacter } from './WndCharacter';
import { WndInventory } from './WndInventory';
import { WndMotion } from './WndMotion';
import type { WndStateConfirm } from './WndStateConfirm';
import { WndStatus } from './WndStatus';

export * from './ports';
export { WndInventory } from './WndInventory';
export { WndStatus } from './WndStatus';
export { WndCharacter } from './WndCharacter';
export { WndMotion } from './WndMotion';
export type { WndStateConfirm } from './WndStateConfirm';
export { CharInfoPage } from './CharInfoPage';
export { MotionPage, EmoticonPage } from './MotionPages';

export interface CharServiceDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  /** `getText(tid)` of the texts bundle (`''` while it is missing). */
  readonly getText: (tid: number) => string;
}

const EMPTY_INVENTORY: InventoryView = {
  bag: [],
  equip: [],
  gold: 0,
  hideCoat: [false, false, false, false, false],
  viewMask: false,
  sorting: false,
  hideFashion: false,
  newFontColor: true,
  version: 0,
  upgradeMode: false,
};
const EMPTY_STATUS: StatusView = {
  title: '',
  hp: 0,
  hpMax: 0,
  mp: 0,
  mpMax: 0,
  fp: 0,
  fpMax: 0,
  expPercentX100: 0,
  restPoint: 0,
  restText: false,
  improved: true,
  expStopFeature: true,
  expStopped: false,
};
const EMPTY_CHARACTER: CharacterView = {
  name: '',
  job: '',
  level: 1,
  baseJob: true,
  legend: 0,
  server: '',
  channel: '',
  str: { total: 0, base: 0 },
  sta: { total: 0, base: 0 },
  dex: { total: 0, base: 0 },
  int: { total: 0, base: 0 },
  remainGp: 0,
  derived: { atk: 0, def: 0, critical: 0, attackSpeed: 0 },
  preview: () => null,
  statistics: false,
  fameName: '',
  fame: 0,
  pkValue: 0,
  pkPropensity: 0,
};
const EMPTY_MOTION: MotionView = { motions: [], emoticons: [], tooltip: (m) => m.description };

/** The honor tab before 45 installs the real ports: no title, actions that do nothing. */
const NULL_HONOR_PORTS: HonorPorts = {
  view: constantHonor(EMPTY_HONOR_VIEW),
  actions: { opened: () => undefined, change: () => undefined },
};

/** The ports before 45 installs the real ones: empty views, actions that do nothing. */
export const NULL_CORE_PORTS: CoreWindowPorts = {
  inventory: {
    view: constantView(EMPTY_INVENTORY),
    actions: {
      useItem: () => undefined,
      unequip: () => undefined,
      unequipByDrag: () => undefined,
      equip: () => undefined,
      moveItem: () => undefined,
      dropIn: () => false,
      trash: () => false,
      sort: () => undefined,
      hideCoat: () => undefined,
      viewMask: () => undefined,
    },
  },
  status: { view: constantView(EMPTY_STATUS), actions: { expStop: () => undefined } },
  character: { view: constantView(EMPTY_CHARACTER), actions: { modifyStatus: () => undefined } },
  motion: {
    view: constantView(EMPTY_MOTION),
    actions: { execute: () => undefined, sendEmoticon: () => undefined },
  },
  previews: null,
};

export class CharService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #atlases: UiAtlases;
  readonly #getText: (tid: number) => string;
  #ports: CoreWindowPorts = NULL_CORE_PORTS;

  constructor(deps: CharServiceDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#atlases = deps.atlases;
    this.#getText = deps.getText;
    const applets = deps.wm.applets;
    applets.register({ app: APP_INVENTORY, factory: () => this.createInventory() });
    applets.register({ app: APP_STATUS1, factory: () => this.createStatus() });
    applets.register({ app: APP_CHARACTER3, factory: () => this.createCharacter() });
    applets.register({ app: APP_MOTION, factory: () => this.createMotion() });
  }

  /** Installs the ports of 45 (any subset; the rest keeps its previous value). */
  setPorts(ports: Partial<CoreWindowPorts>): void {
    this.#ports = { ...this.#ports, ...ports };
  }

  get ports(): CoreWindowPorts {
    return this.#ports;
  }

  // ---- window factories (also used by the gallery and the tests) ---------------------------------------------------------

  createInventory(): WndInventory {
    const p = this.#ports;
    return new WndInventory({
      view: p.inventory.view,
      actions: p.inventory.actions,
      tooltip: this.#tooltip,
      getText: this.#getText,
      previews: p.previews ?? null,
    });
  }

  createStatus(): WndStatus {
    const p = this.#ports;
    return new WndStatus({
      view: p.status.view,
      actions: p.status.actions,
      tooltip: this.#tooltip,
      getText: this.#getText,
      previews: p.previews ?? null,
    });
  }

  createCharacter(): WndCharacter {
    const p = this.#ports;
    return new WndCharacter({
      view: p.character.view,
      actions: p.character.actions,
      tooltip: this.#tooltip,
      getText: this.#getText,
      openStateConfirm: (r) => {
        this.openStateConfirm(r);
      },
      honor: honorDeps(p.honor ?? NULL_HONOR_PORTS, this.#getText),
    });
  }

  createMotion(): WndMotion {
    const p = this.#ports;
    return new WndMotion({
      view: p.motion.view,
      actions: p.motion.actions,
      tooltip: this.#tooltip,
      atlases: this.#atlases,
      getText: this.#getText,
    });
  }

  /** Opens (or focuses) each window through the window manager; the gallery and the tests use these. */
  openInventory(): WndInventory | null {
    return this.#wm.open(APP_INVENTORY);
  }
  openStatus(): WndStatus | null {
    return this.#wm.open(APP_STATUS1);
  }
  openCharacter(): WndCharacter | null {
    return this.#wm.open(APP_CHARACTER3);
  }
  openMotion(): WndMotion | null {
    return this.#wm.open(APP_MOTION);
  }

  // ---- APP_STATE_CONFIRM -------------------------------------------------------------------------------------------------

  /**
   * `SAFE_DELETE(m_pWndStateConfirm); m_pWndStateConfirm = new CWndStateConfirm; Initialize()`: a second request replaces the
   * first window.
   */
  openStateConfirm(request: StateConfirmRequest): WndStateConfirm | null {
    return openLazy(
      this.#wm,
      APP_STATE_CONFIRM,
      CHAR_LAZY.WndStateConfirm,
      (C) => new C(request),
      (app) => {
        const old = this.#wm.get(app);
        if (!old) return;
        old.destroy();
        this.#wm.flushDestroyed();
      },
    );
  }

  /** The open stat confirm window, `null` when none. */
  get stateConfirm(): WndStateConfirm | null {
    const w = this.#wm.get(APP_STATE_CONFIRM);
    const ctor = CHAR_LAZY.WndStateConfirm.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /** 43's key router (`KillWindow`): Enter = Yes, Esc = close; both do nothing without the window. */
  readonly statConfirm: { yes(): void; cancel(): void } = {
    yes: () => {
      this.stateConfirm?.yes();
    },
    cancel: () => {
      this.stateConfirm?.cancel();
    },
  };
}
