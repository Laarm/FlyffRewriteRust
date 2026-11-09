// `SkillWindowsService` (`ui.skills`, Plan/42 §19.4 D; WP-UI-38): the skill tree and its helper windows.
//
//   ui.skills.setPorts({ tree })               45 WP-CFEAT-31: the view and the actions of `APP_SKILL3`
//   ui.skills.openTree()                       APP_SKILL3 (the applet: hotkey K, the start menu, the taskbar)
//   ui.skills.openReSkillControl(request)      APP_RESKILL_CONTROL1 "Skill Distribution" (the original never opens it)
//   ui.skills.openHeroSkillUp(request)         APP_HERO_SKILLUP: five gems for the hero skill (MMI_LEGEND_SKILLUP)
//   ui.skills.heroSkillUp()                    the open hero skill-up window (the inventory feeds it gems; `receiveResult`)
//
// The service registers the `APP_SKILL3` applet so that the taskbar, the start menu and the hotkey open it; the factory reads the
// ports at the moment a window is created, so ports installed later reach windows opened later.

import { APP_HERO_SKILLUP, APP_RESKILL_CONTROL1, APP_SKILL3 } from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type {
  HeroSkillUpRequest,
  ReSkillControlRequest,
  SkillTreeActions,
  SkillTreeView,
  ViewSource,
} from './ports';
import { SKILLS_LAZY } from './lazy';
import type { WndHeroSkillUp } from './WndHeroSkillUp';
import type { WndReSkillControl } from './WndReSkillControl';
import { WndSkillTree } from './WndSkillTree';

export * from './ports';
export { WndSkillTree, LEVEL_ATLAS, ARROW_OPEN, ARROW_FOLDED } from './WndSkillTree';
export type { WndReSkillControl } from './WndReSkillControl';
export type { WndHeroSkillUp } from './WndHeroSkillUp';
export * from './treeGeometry';

export interface SkillPorts {
  readonly tree: { view: ViewSource<SkillTreeView>; actions: SkillTreeActions };
}

export interface SkillWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
}

const EMPTY_VIEW: SkillTreeView = {
  job: -1,
  legend: false,
  titles: ['', '', ''],
  panels: [null, null],
  heroBackground: null,
  slots: [],
  sp: 0,
  serverSp: 0,
  selected: -1,
  buttons: { add: false, sub: false, reset: false, finish: false },
  level: 1,
};

/** The ports before 45 installs the real ones: an empty tree, actions that do nothing. */
export const NULL_SKILL_PORTS: SkillPorts = {
  tree: {
    view: { get: () => EMPTY_VIEW, subscribe: () => () => undefined },
    actions: {
      select: () => undefined,
      add: () => undefined,
      sub: () => undefined,
      reset: () => undefined,
      finish: () => undefined,
      queue: () => undefined,
    },
  },
};

export class SkillWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #atlases: UiAtlases;
  #ports: SkillPorts = NULL_SKILL_PORTS;

  constructor(deps: SkillWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#atlases = deps.atlases;
    deps.wm.applets.register({ app: APP_SKILL3, factory: () => this.createTree() });
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<SkillPorts>): void {
    this.#ports = { ...this.#ports, ...ports };
  }

  get ports(): SkillPorts {
    return this.#ports;
  }

  // ---- APP_SKILL3 ------------------------------------------------------------------------------------------------------------

  createTree(): WndSkillTree {
    const p = this.#ports.tree;
    return new WndSkillTree({
      view: p.view,
      actions: p.actions,
      tooltip: this.#tooltip,
      atlases: this.#atlases,
    });
  }

  openTree(): WndSkillTree | null {
    return this.#wm.open(APP_SKILL3);
  }

  /** The open skill tree, `null` when none. */
  tree(): WndSkillTree | null {
    const w = this.#wm.get(APP_SKILL3);
    return w instanceof WndSkillTree ? w : null;
  }

  // ---- APP_RESKILL_CONTROL1 ------------------------------------------------------------------------------------------------

  openReSkillControl(request: ReSkillControlRequest): WndReSkillControl | null {
    return this.#replace(
      APP_RESKILL_CONTROL1,
      SKILLS_LAZY.WndReSkillControl,
      (C) => new C(request),
    );
  }

  // ---- APP_HERO_SKILLUP ------------------------------------------------------------------------------------------------------

  /** `MMI_LEGEND_SKILLUP`: the window opens once (the caller prints `TID_GAME_WND_OVERLAPED` for a second request). */
  openHeroSkillUp(request: HeroSkillUpRequest): WndHeroSkillUp | null {
    return this.#replace(APP_HERO_SKILLUP, SKILLS_LAZY.WndHeroSkillUp, (C) => new C(request));
  }

  heroSkillUp(): WndHeroSkillUp | null {
    const w = this.#wm.get(APP_HERO_SKILLUP);
    const ctor = SKILLS_LAZY.WndHeroSkillUp.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  isHeroSkillUpOpen(): boolean {
    return this.heroSkillUp() !== null;
  }

  /** Opens the window of a lazy class, replacing the open one (`null` while its chunk is still loading: the window then opens by itself). */
  #replace<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      const old = this.#wm.get(a);
      if (!old) return;
      old.destroy();
      this.#wm.flushDestroyed();
    });
  }
}
