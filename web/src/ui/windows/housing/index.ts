// `HousingWindowsService` (`ui.housing`, Plan/42 §19.4 N, 45 §28.10, §28.11; WP-UI-58, WP-UI-59): the windows of the personal mini room and of the guild house.
//
//   ui.housing.setHousingPort(port)        the port of the applet APP_HOUSING (the furniture list; opened from the full start menu, `features.clean_start_menu = false`)
//   ui.housing.setStoragePort(port)        the port of the applet APP_GH_FURNITURE_STORAGE (the furniture and teleporter storage)
//   ui.housing.openRoomList(request)       APP_MINIROOM_LIST: the rooms of the friends (menu 123); replaces an open one
//   ui.housing.openQuitRoom(request)       APP_QUIT_ROOM: leave the mini room (menu 124)
//   ui.housing.openMain(request) / closeMain()   APP_GH_MAIN: the guild-house menu (opened with the world 212-214, no close button, Esc keeps it)
//   ui.housing.openStorage(section) / toggleStorage(section) / storage()   the storage window in 'furniture' or 'teleporter'; `toggleStorage` closes it when it already shows that section
//   ui.housing.openShowOne(row) / closeShowOne()   APP_GH_SHOWONE: the item being placed (the storage window hides meanwhile)
//   ui.housing.openUpkeep(request) / closeUpkeep() / upkeep()   the upkeep box on APP_CONFIRM_BUYEX under the instance key 'ghUpkeep'
//   ui.housing.openTeleporter(request)     APP_GH_TELEPORTER: the five-slot shell V15 never instantiates
//   ui.housing.refresh(app)                the window of `app` re-reads its port (the housing list, the room list, the storage)
//   ui.housing.syncDeploy()                the storage window follows the placement mode (hides / shows)
//   ui.housing.isOpen(app) / closeAll()

import {
  APP_CONFIRM_BUYEX,
  APP_GH_FURNITURE_STORAGE,
  APP_GH_MAIN,
  APP_GH_SHOWONE,
  APP_GH_TELEPORTER,
  APP_HOUSING,
  APP_MINIROOM_LIST,
  APP_QUIT_ROOM,
} from '../../../gen/defs/resdata';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { HOUSING_LAZY } from './lazy';
import type {
  GhMainRequest,
  HousingHost,
  HousingPort,
  QuitRoomRequest,
  RoomListRequest,
  StoragePort,
  TeleporterRequest,
  UpkeepRequest,
} from './ports';
import { type GhRowView, type GhSection, UPKEEP_KEY } from './rows';
import type { WndGHMainMenu } from './WndGHMainMenu';
import type { WndGHouseShowOneUnit } from './WndGHouseShowOneUnit';
import type { WndGHTeleporter } from './WndGHTeleporter';
import type { WndGHUpkeep } from './WndGHUpkeep';
import { WndGuildHousing } from './WndGuildHousing';
import { WndHousing } from './WndHousing';
import type { WndQuitRoom } from './WndQuitRoom';
import type { WndRoomList } from './WndRoomList';

export * from './ports';
export * from './rows';
export type { HouseCell, HouseRow } from './HouseList';
export { WndGuildHousing, WndHousing };
export type {
  WndGHMainMenu,
  WndGHouseShowOneUnit,
  WndGHTeleporter,
  WndGHUpkeep,
  WndQuitRoom,
  WndRoomList,
};

export interface HousingWindowDeps {
  readonly wm: WindowManager;
  readonly getText: (tid: number) => string;
  /** Unix seconds now (the SHOWONE strip counts with it; the storage port has its own). */
  readonly nowSec?: () => number;
}

/** The `APP_*` of every window of the family (`closeAll`). */
export const HOUSING_APPS: readonly number[] = [
  APP_HOUSING,
  APP_MINIROOM_LIST,
  APP_QUIT_ROOM,
  APP_GH_MAIN,
  APP_GH_FURNITURE_STORAGE,
  APP_GH_TELEPORTER,
  APP_GH_SHOWONE,
];

export class HousingWindowsService {
  readonly #wm: WindowManager;
  readonly #host: HousingHost;
  readonly #nowSec: () => number;
  #housing: HousingPort | null = null;
  #storage: StoragePort | null = null;

  constructor(deps: HousingWindowDeps) {
    this.#wm = deps.wm;
    this.#nowSec = deps.nowSec ?? (() => Math.floor(Date.now() / 1000));
    this.#host = { text: deps.getText, box: () => undefined };
    // the two applets: the start menu (full menu), the keyword `/open` and the taskbar see them from now on; the ports arrive with the feature
    deps.wm.applets.register({
      app: APP_HOUSING,
      factory: () => new WndHousing({ port: () => this.#housing, host: this.#host }),
    });
    deps.wm.applets.register({
      app: APP_GH_FURNITURE_STORAGE,
      factory: () =>
        new WndGuildHousing({
          port: () => this.#storage,
          host: this.#host,
          showOne: {
            open: (row) => {
              this.openShowOne(row);
            },
            close: () => {
              this.closeShowOne();
            },
          },
        }),
    });
  }

  /** The texts the shells use (tests). */
  get host(): HousingHost {
    return this.#host;
  }

  setHousingPort(port: HousingPort | null): void {
    this.#housing = port;
  }
  setStoragePort(port: StoragePort | null): void {
    this.#storage = port;
  }

  // ---- helpers -----------------------------------------------------------------------------------------------------------------

  #live(app: number, key = ''): WndNeuz | null {
    const w = this.#wm.get(app, key);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `SAFE_DELETE( pWnd )`: the old window of the app goes at once. */
  #replaceApp(app: number, key = ''): void {
    const old = this.#wm.get(app, key);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
    key = '',
  ): InstanceType<C> | null {
    const ctor = lazy.ctor;
    if (key === '') {
      return openLazy(this.#wm, app, lazy, build, (a) => {
        this.#replaceApp(a);
      });
    }
    // an instance under a key: replace that instance only
    const open = (c: C): InstanceType<C> | null => {
      this.#replaceApp(app, key);
      return this.#wm.open(app, () => build(c), key);
    };
    if (ctor) return open(ctor);
    void lazy.load().then(open, () => undefined);
    return null;
  }

  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>, key = ''): InstanceType<C> | null {
    const w = this.#live(app, key);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  // ---- the mini room ---------------------------------------------------------------------------------------------------------

  /** `MMI_VISIT_FRIEND`: `SAFE_DELETE( m_pWndRoomList ); new CWndRoomList` (the old one goes first). */
  openRoomList(request: RoomListRequest): WndRoomList | null {
    return this.#open(APP_MINIROOM_LIST, HOUSING_LAZY.WndRoomList, (C) => new C(request));
  }
  roomList(): WndRoomList | null {
    return this.#get(APP_MINIROOM_LIST, HOUSING_LAZY.WndRoomList);
  }

  /** `MMI_RETURNTO_WORLD`: `SAFE_DELETE( m_pWndQuitRoom ); new CWndQuitRoom`. */
  openQuitRoom(request: QuitRoomRequest): WndQuitRoom | null {
    return this.#open(APP_QUIT_ROOM, HOUSING_LAZY.WndQuitRoom, (C) => new C(request));
  }
  quitRoom(): WndQuitRoom | null {
    return this.#get(APP_QUIT_ROOM, HOUSING_LAZY.WndQuitRoom);
  }

  /** The housing applet (`GetApplet( APP_HOUSING )`). */
  housing(): WndHousing | null {
    const w = this.#live(APP_HOUSING);
    return w instanceof WndHousing ? w : null;
  }

  // ---- the guild house -------------------------------------------------------------------------------------------------------

  /** The guild-house menu (opened with the world); a second call keeps the open one. */
  openMain(request: GhMainRequest): WndGHMainMenu | null {
    const ctor = HOUSING_LAZY.WndGHMainMenu.ctor;
    const open = (C: NonNullable<typeof ctor>): WndGHMainMenu | null =>
      this.#wm.open(APP_GH_MAIN, () => new C(request, this.#host));
    if (ctor) return open(ctor);
    void HOUSING_LAZY.WndGHMainMenu.load().then(open, () => undefined);
    return null;
  }
  main(): WndGHMainMenu | null {
    return this.#get(APP_GH_MAIN, HOUSING_LAZY.WndGHMainMenu);
  }
  closeMain(): void {
    this.#live(APP_GH_MAIN)?.destroy();
  }

  /** The storage window (`GetApplet( APP_GH_FURNITURE_STORAGE )`). */
  storage(): WndGuildHousing | null {
    const w = this.#live(APP_GH_FURNITURE_STORAGE);
    return w instanceof WndGuildHousing ? w : null;
  }

  /** Opens the storage window in a section, or switches an open one to it (`InitBySection`). */
  openStorage(section: GhSection): WndGuildHousing | null {
    let w = this.storage();
    if (!w) {
      w = this.#wm.open<WndGuildHousing>(APP_GH_FURNITURE_STORAGE);
      if (!w) return null;
      w.initBySection(section);
    } else if (!w.isSection(section)) {
      w.initBySection(section);
    }
    return w;
  }

  /** `BTN_FURNITURE` / `BTN_TELEPORTER`: the window of that section closes, the other section switches, a closed window opens. */
  toggleStorage(section: GhSection): WndGuildHousing | null {
    const w = this.storage();
    if (w?.isSection(section) === true) {
      w.destroy();
      return null;
    }
    return this.openStorage(section);
  }

  /** `new CWndGHouseShowOneUnit; SetItem` (replaces an open one). */
  openShowOne(row: GhRowView): WndGHouseShowOneUnit | null {
    return this.#open(
      APP_GH_SHOWONE,
      HOUSING_LAZY.WndGHouseShowOneUnit,
      (C) =>
        new C(row, {
          host: this.#host,
          nowSec: this.#storage?.nowSec.bind(this.#storage) ?? this.#nowSec,
        }),
    );
  }
  showOne(): WndGHouseShowOneUnit | null {
    return this.#get(APP_GH_SHOWONE, HOUSING_LAZY.WndGHouseShowOneUnit);
  }
  closeShowOne(): void {
    this.#live(APP_GH_SHOWONE)?.destroy();
  }

  /** The upkeep box (`SAFE_DELETE` then `new`): under its own instance key, so a shop's quantity box (306) stays. */
  openUpkeep(request: UpkeepRequest): WndGHUpkeep | null {
    return this.#open(
      APP_CONFIRM_BUYEX,
      HOUSING_LAZY.WndGHUpkeep,
      (C) => new C(request, this.#host),
      UPKEEP_KEY,
    );
  }
  upkeep(): WndGHUpkeep | null {
    return this.#get(APP_CONFIRM_BUYEX, HOUSING_LAZY.WndGHUpkeep, UPKEEP_KEY);
  }
  closeUpkeep(): void {
    this.#live(APP_CONFIRM_BUYEX, UPKEEP_KEY)?.destroy();
  }
  isUpkeepOpen(): boolean {
    return this.#live(APP_CONFIRM_BUYEX, UPKEEP_KEY) !== null;
  }

  /** The five-slot teleporter shell. */
  openTeleporter(request: TeleporterRequest): WndGHTeleporter | null {
    return this.#open(
      APP_GH_TELEPORTER,
      HOUSING_LAZY.WndGHTeleporter,
      (C) => new C(request, this.#host),
    );
  }
  teleporter(): WndGHTeleporter | null {
    return this.#get(APP_GH_TELEPORTER, HOUSING_LAZY.WndGHTeleporter);
  }

  // ---- refreshes ---------------------------------------------------------------------------------------------------------------

  /** `RefreshItemList()` / `Refresh()` of the window of `app` (nothing when it is not open). */
  refresh(app: number): void {
    if (app === APP_HOUSING) this.housing()?.refresh();
    else if (app === APP_MINIROOM_LIST) this.roomList()?.refresh();
    else if (app === APP_GH_FURNITURE_STORAGE) {
      const w = this.storage();
      w?.refresh();
      w?.syncDeploy();
    } else if (app === APP_GH_TELEPORTER) this.teleporter()?.refresh();
  }

  /** The storage window follows the placement mode (hidden frames are not processed, so the feature tells). */
  syncDeploy(): void {
    this.storage()?.syncDeploy();
  }

  /** Every window of the family goes (the session left the world). */
  closeAll(): void {
    for (const app of HOUSING_APPS) this.#live(app)?.destroy();
    this.#live(APP_CONFIRM_BUYEX, UPKEEP_KEY)?.destroy();
  }
}
