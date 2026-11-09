// `QuestWindowsService` (`ui.quest`, Plan/42 §19.4 G, 45 §28.1; WP-UI-53): the quest windows.
//
//   ui.quest.enableApplet( ports )            registers the factory of `APP_QUEST_EX_LIST` (the list applet): the taskbar, the `Q` key, the applet keyword and the start menu see it from now on.
//                                             Until it runs the applet is PENDING and nothing opens (PL-148: the shipped data has no quests)
//   ui.quest.openDetail( request )            862: the detail of one quest next to the list (replaces the open one); closeDetail() / detail()
//   ui.quest.openQuickInfo( request )         857: the tracker (one; visible while a quest is ticked); quickInfo() / closeQuickInfo()
//   ui.quest.openHeavenTower( request )       734: the floor list of the NPC menu; null while the chunk loads / the layout is missing; heavenTower()
//   ui.quest.isOpen( app ) / openApps()       whether a quest window is open / the open ones
//   ui.quest.closeAll()                       every quest window goes
//
// The Heaven Tower contract (ui-secret-tax, WP-CFEAT-52): `openHeavenTower( { enter( floor ) } )` with the rows, texts, highlight and confirmation inside the shell, `enter` for the floors 1..5 only,
// the costs in `HEAVEN_TOWER_COSTS`, the selected row in `heavenTower()?.chosen`.

import {
  APP_HEAVEN_TOWER,
  APP_QUEST_EX_DETAIL,
  APP_QUEST_EX_LIST,
  APP_QUEST_QUICK_INFO,
} from '../../../gen/defs/resdata';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import { MB, MBID } from '../../msgbox/types';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { QUEST_LAZY } from './lazy';
import type {
  HeavenTowerRequest,
  QuestDetailRequest,
  QuestHost,
  QuestListPorts,
  QuestQuickInfoRequest,
} from './ports';
import type { WndHeavenTower } from './WndHeavenTower';
import { WndQuest } from './WndQuest';
import type { WndQuestDetail } from './WndQuestDetail';
import type { WndQuestQuickInfo } from './WndQuestQuickInfo';

export * from './ports';
export * from './rows';
export * from './colors';
export { WndQuest } from './WndQuest';
export type { WndHeavenTower, WndQuestDetail, WndQuestQuickInfo };

export interface QuestWindowDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  readonly getText: (tid: number) => string;
}

/** The `APP_*` of every window of the family (`closeAll`). */
export const QUEST_APPS: readonly number[] = [
  APP_QUEST_EX_LIST,
  APP_QUEST_EX_DETAIL,
  APP_QUEST_QUICK_INFO,
  APP_HEAVEN_TOWER,
];

/** `%s` / `%d` of a legacy template, in order. */
function fmt(template: string, args: readonly (string | number)[]): string {
  let i = 0;
  return template.replace(/%(?:%|[-+ #0]*\d*(?:\.\d+)?(?:I64|ll|l|h)?[sducfxXi])/g, (m) => {
    if (m === '%%') return '%';
    const a = args[i];
    i += 1;
    return a === undefined ? '' : String(a);
  });
}

export class QuestWindowsService {
  readonly #wm: WindowManager;
  readonly #host: QuestHost;
  #ports: QuestListPorts | null = null;
  #enabled = false;

  constructor(deps: QuestWindowDeps) {
    this.#wm = deps.wm;
    const { msgbox, getText } = deps;
    this.#host = {
      text: getText,
      fmt: (tid, ...args) => fmt(getText(tid), args),
      box: (text) => {
        msgbox.open(text);
      },
      boxWithTitle: (title, text) => {
        msgbox.openWithTitle(title, text);
      },
      confirm: (text, onOk, owner) => {
        msgbox.open(text, MB.OKCANCEL, (owner as WndNeuz | undefined) ?? undefined, (id) => {
          if (id === MBID.OK) onOk();
        });
      },
    };
  }

  /** The texts and boxes the shells use (tests). */
  get host(): QuestHost {
    return this.#host;
  }

  // ---- the applet -------------------------------------------------------------------------------------------------------------------

  /**
   * PL-148: registers the factory of the list applet. Called once, by the quest feature, when quest data is loaded (`quests.length > 0`); `isAvailable( APP_QUEST_EX_LIST )` is true from then on.
   * A second call replaces the ports.
   */
  enableApplet(ports: QuestListPorts): void {
    this.#ports = ports;
    if (this.#enabled) return;
    this.#enabled = true;
    this.#wm.applets.register({
      app: APP_QUEST_EX_LIST,
      factory: () => this.createList(),
    });
  }

  /** The applet was enabled. */
  get appletEnabled(): boolean {
    return this.#enabled;
  }

  /** `new CWndQuest` (the registry factory). */
  createList(): WndQuest {
    return new WndQuest({
      host: this.#host,
      ports: () => {
        if (!this.#ports) throw new Error('the quest list has no ports');
        return this.#ports;
      },
      detailId: () => this.detail()?.questId ?? null,
      openDetail: (request) => {
        this.openDetail(request);
      },
      closeDetail: () => {
        this.closeDetail();
      },
    });
  }

  /** The open list applet. */
  list(): WndQuest | null {
    const w = this.#wm.get(APP_QUEST_EX_LIST);
    return w instanceof WndQuest ? w : null;
  }

  // ---- helpers -----------------------------------------------------------------------------------------------------------------------

  /** `GetApplet( app )` that is not on its way out. */
  #live(app: number): WndNeuz | null {
    const w = this.#wm.get(app);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

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
    const w = this.#live(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  /** The `APP_*` of the quest windows that are open now (the debug probe). */
  openApps(): number[] {
    return QUEST_APPS.filter((app) => this.isOpen(app));
  }

  // ---- the windows --------------------------------------------------------------------------------------------------------------------

  /** `SAFE_DELETE( g_WndMng.m_pWndQuestDetail ); new CWndQuestDetail( id ); Initialize()`. */
  openDetail(request: QuestDetailRequest): WndQuestDetail | null {
    return this.#open(APP_QUEST_EX_DETAIL, QUEST_LAZY.WndQuestDetail, (C) => new C(request));
  }

  detail(): WndQuestDetail | null {
    return this.#get(APP_QUEST_EX_DETAIL, QUEST_LAZY.WndQuestDetail);
  }

  closeDetail(): void {
    this.#live(APP_QUEST_EX_DETAIL)?.destroy();
  }

  /** `CWndWorld::OpenField`: the quick info (one; a second call replaces the first). */
  openQuickInfo(request: QuestQuickInfoRequest): WndQuestQuickInfo | null {
    return this.#open(APP_QUEST_QUICK_INFO, QUEST_LAZY.WndQuestQuickInfo, (C) => new C(request));
  }

  quickInfo(): WndQuestQuickInfo | null {
    return this.#get(APP_QUEST_QUICK_INFO, QUEST_LAZY.WndQuestQuickInfo);
  }

  closeQuickInfo(): void {
    this.#live(APP_QUEST_QUICK_INFO)?.destroy();
  }

  /** `MMI_HEAVEN_TOWER`: `new CWndHeavenTower` (replaces an open one). */
  openHeavenTower(request: HeavenTowerRequest): WndHeavenTower | null {
    return this.#open(
      APP_HEAVEN_TOWER,
      QUEST_LAZY.WndHeavenTower,
      (C) => new C(request, { host: this.#host }),
    );
  }

  heavenTower(): WndHeavenTower | null {
    return this.#get(APP_HEAVEN_TOWER, QUEST_LAZY.WndHeavenTower);
  }

  /** Every quest window goes (the session left the world). */
  closeAll(): void {
    for (const app of QUEST_APPS) this.#live(app)?.destroy();
  }
}
