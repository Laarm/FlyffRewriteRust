// `LordWindowsService` (`ui.lord`, Plan/42 §19.4 K / O; WP-UI-54, WP-UI-55): the lord windows - the election (state, tender, pledge, confirm, vote), the lord's event window, the lord skill APPLET with its name
// prompt, and the two information windows.
//
//   ui.lord.setPorts({ lord })                  45 WP-CFEAT-43: the view and the actions of the windows (the NPC menu entries 91-94, 101, 102 only open)
//   ui.lord.openState / openTender / openVote   722 / 718 / 724 (91 State, 92 Tender, 93 Vote)       ui.lord.openEvent()           726 (94)
//   ui.lord.openInfo / openRPInfo               756 / 758 (101, 102)                                  ui.lord.openPledge( rank )    720 (a click in 722; opens only from there in the original)
//   ui.lord.openSkillConfirm( skill )           736 (LI_SUMMON / LI_TELEPORT of the lord skill)       ui.lord.openConfirm( request )   738 (the re-pledge of 720)
//   (applet)   APP_LORD_SKILL (728, hotkey L) opens through the applet registry; a non-lord gets the chat line and no window
//   ui.lord.closeAll()                          every lord window goes
//
// Each `open*` replaces the open window of the same kind (the original: `SAFE_DELETE( m_pWndX ); new ...`) and answers `null` while the chunk of the window is still loading (the window then opens by
// itself). The ports are read at the moment a window is created, so ports installed later reach windows opened later.

import {
  APP_LORD_CONFIRM,
  APP_LORD_EVENT,
  APP_LORD_INFO,
  APP_LORD_INFO2,
  APP_LORD_PLEDGE,
  APP_LORD_SKILL,
  APP_LORD_SKILL_CONFIRM,
  APP_LORD_STATE,
  APP_LORD_TENDER,
  APP_LORD_VOTE,
} from '../../../gen/defs/resdata';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { fmt } from '../siege/format';
import { LORD_LAZY } from './lazy';
import {
  constantView,
  EMPTY_LORD_VIEW,
  type LordConfirmRequest,
  type LordHost,
  type LordPorts,
  type LordWindowDeps,
  type LordWindowOps,
} from './ports';
import type { WndLordConfirm } from './WndLordConfirm';
import type { WndLordEvent } from './WndLordEvent';
import type { WndLordInfo } from './WndLordInfo';
import type { WndLordPledge } from './WndLordPledge';
import type { WndLordRPInfo } from './WndLordRPInfo';
import { WndLordSkill } from './WndLordSkill';
import type { WndLordSkillConfirm } from './WndLordSkillConfirm';
import type { WndLordState } from './WndLordState';
import type { WndLordTender } from './WndLordTender';
import type { WndLordVote } from './WndLordVote';

export * from './ports';
export * from './rows';
export { skillWindowTooltip, SKILL_ICON, WndLordSkill } from './WndLordSkill';
export type {
  WndLordConfirm,
  WndLordEvent,
  WndLordInfo,
  WndLordPledge,
  WndLordRPInfo,
  WndLordSkillConfirm,
  WndLordState,
  WndLordTender,
  WndLordVote,
};

export interface LordWindowServiceDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly msgbox: MessageBoxApi;
  readonly getText: (tid: number) => string;
}

/** The `APP_*` of every window of the family (`closeAll`, the probe). */
export const LORD_APPS: readonly number[] = [
  APP_LORD_STATE,
  APP_LORD_TENDER,
  APP_LORD_PLEDGE,
  APP_LORD_VOTE,
  APP_LORD_EVENT,
  APP_LORD_SKILL,
  APP_LORD_SKILL_CONFIRM,
  APP_LORD_CONFIRM,
  APP_LORD_INFO,
  APP_LORD_INFO2,
];

/** The ports before 45 installs the real ones: no lord, no election, actions that do nothing. */
export const NULL_LORD_PORTS: LordPorts = {
  view: constantView(EMPTY_LORD_VIEW),
  actions: {
    addDeposit: () => undefined,
    setPledge: () => undefined,
    incVote: () => undefined,
    createEvent: () => undefined,
    useSkill: () => undefined,
    castSkill: () => undefined,
  },
  nowS: () => Date.now() / 1000,
  panel: () => '',
  say: () => undefined,
  sayText: () => undefined,
};

export class LordWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #host: LordHost;
  #ports: LordPorts = NULL_LORD_PORTS;

  constructor(deps: LordWindowServiceDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    const { msgbox, getText } = deps;
    this.#host = {
      text: getText,
      fmt: (tid, ...args) => fmt(getText(tid), args),
      box: (text) => {
        msgbox.open(text);
      },
    };
    deps.wm.applets.register({ app: APP_LORD_SKILL, factory: () => this.createSkill() });
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<{ lord: LordPorts }>): void {
    if (ports.lord) this.#ports = ports.lord;
  }

  get ports(): LordPorts {
    return this.#ports;
  }

  /** The texts and boxes the shells use (tests). */
  get host(): LordHost {
    return this.#host;
  }

  // ---- helpers -----------------------------------------------------------------------------------------------------------

  #deps(): LordWindowDeps {
    return { ports: this.#ports, host: this.#host };
  }

  readonly #ops: LordWindowOps = {
    openPledge: (rank) => {
      this.openPledge(rank);
    },
    openConfirm: (request) => {
      this.openConfirm(request);
    },
    closePledge: () => {
      this.#wm.get(APP_LORD_PLEDGE)?.destroy();
      this.#wm.get(APP_LORD_CONFIRM)?.destroy();
    },
    closeConfirm: () => {
      this.#wm.get(APP_LORD_CONFIRM)?.destroy();
    },
  };

  /** `SAFE_DELETE( pWnd )`: the old window of the app goes at once. */
  #replaceApp(app: number): void {
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
      this.#replaceApp(a);
    });
  }

  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>): InstanceType<C> | null {
    const w = this.#wm.get(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor && !this.#wm.isDestroying(w) ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    const w = this.#wm.get(app);
    return w !== null && !this.#wm.isDestroying(w);
  }

  /** The `APP_*` of the lord windows that are open now (the debug probe). */
  openApps(): number[] {
    return LORD_APPS.filter((app) => this.isOpen(app));
  }

  // ---- the windows ---------------------------------------------------------------------------------------------------------

  /** `new CWndLordState; Initialize( &g_WndMng )` (menu 91). */
  openState(): WndLordState | null {
    return this.#open(
      APP_LORD_STATE,
      LORD_LAZY.WndLordState,
      (C) => new C(this.#deps(), this.#ops),
    );
  }
  state(): WndLordState | null {
    return this.#get(APP_LORD_STATE, LORD_LAZY.WndLordState);
  }

  /** Menu 92. */
  openTender(): WndLordTender | null {
    return this.#open(APP_LORD_TENDER, LORD_LAZY.WndLordTender, (C) => new C(this.#deps()));
  }
  tender(): WndLordTender | null {
    return this.#get(APP_LORD_TENDER, LORD_LAZY.WndLordTender);
  }

  /** The pledge of the candidate of `rank` (a click on a row of the state window). */
  openPledge(rank: number): WndLordPledge | null {
    return this.#open(
      APP_LORD_PLEDGE,
      LORD_LAZY.WndLordPledge,
      (C) => new C(rank, this.#deps(), this.#ops),
    );
  }
  pledge(): WndLordPledge | null {
    return this.#get(APP_LORD_PLEDGE, LORD_LAZY.WndLordPledge);
  }

  /** The confirmation of a changed pledge (`new CWndLordConfirm( pledge ); Initialize()`). */
  openConfirm(request: LordConfirmRequest): WndLordConfirm | null {
    return this.#open(
      APP_LORD_CONFIRM,
      LORD_LAZY.WndLordConfirm,
      (C) => new C(request, this.#deps(), this.#ops),
    );
  }
  confirm(): WndLordConfirm | null {
    return this.#get(APP_LORD_CONFIRM, LORD_LAZY.WndLordConfirm);
  }

  /** Menu 93. */
  openVote(): WndLordVote | null {
    return this.#open(APP_LORD_VOTE, LORD_LAZY.WndLordVote, (C) => new C(this.#deps()));
  }
  vote(): WndLordVote | null {
    return this.#get(APP_LORD_VOTE, LORD_LAZY.WndLordVote);
  }

  /** Menu 94. A character that may not use it gets the chat line and the window closes itself. */
  openEvent(): WndLordEvent | null {
    return this.#open(APP_LORD_EVENT, LORD_LAZY.WndLordEvent, (C) => new C(this.#deps()));
  }
  event(): WndLordEvent | null {
    return this.#get(APP_LORD_EVENT, LORD_LAZY.WndLordEvent);
  }

  /** The name prompt of the lord skill `skill` (`LI_SUMMON` or `LI_TELEPORT`). */
  openSkillConfirm(skill: number): WndLordSkillConfirm | null {
    return this.#open(
      APP_LORD_SKILL_CONFIRM,
      LORD_LAZY.WndLordSkillConfirm,
      (C) => new C(skill, this.#deps()),
    );
  }
  skillConfirm(): WndLordSkillConfirm | null {
    return this.#get(APP_LORD_SKILL_CONFIRM, LORD_LAZY.WndLordSkillConfirm);
  }

  /** Menu 101. */
  openInfo(): WndLordInfo | null {
    return this.#open(APP_LORD_INFO, LORD_LAZY.WndLordInfo, (C) => new C(this.#deps()));
  }
  info(): WndLordInfo | null {
    return this.#get(APP_LORD_INFO, LORD_LAZY.WndLordInfo);
  }

  /** Menu 102. */
  openRPInfo(): WndLordRPInfo | null {
    return this.#open(APP_LORD_INFO2, LORD_LAZY.WndLordRPInfo, (C) => new C(this.#deps()));
  }
  rpInfo(): WndLordRPInfo | null {
    return this.#get(APP_LORD_INFO2, LORD_LAZY.WndLordRPInfo);
  }

  // ---- the applet --------------------------------------------------------------------------------------------------------

  createSkill(): WndLordSkill {
    return new WndLordSkill({ ...this.#deps(), tooltip: this.#tooltip });
  }

  /** The open lord skill window. */
  skill(): WndLordSkill | null {
    const w = this.#wm.get(APP_LORD_SKILL);
    return w instanceof WndLordSkill && !this.#wm.isDestroying(w) ? w : null;
  }

  /** Every lord window goes (the character left the world; the field windows of the window manager go the same way). */
  closeAll(): void {
    for (const app of LORD_APPS) this.#wm.get(app)?.destroy();
    this.#wm.flushDestroyed();
  }
}
