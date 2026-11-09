// The start menu (`CWndTaskMenu`, WndTaskBar.cpp:2617-2809; Plan/42 §11.4; WP-UI-14): a popup frame on
// `Theme/WndTaskMenu.tga` (frame size = texture size) with items at `(10, 50 + i * 22, texW - 20, 70 + i * 22)`. Each
// applet item shows the applet icon and tooltip. It opens at `x = 0`, `y = layoutBottom - menuHeight` and hides when it
// loses the focus.
//
// `buildStartMenu` is the pure item list of both variants (`features.clean_start_menu` true and false); entries whose
// window is not built yet are left out by the `exists` predicate (catalog `built` flag, §11.4).

import {
  APP_BAG_EX,
  APP_CHARACTER3,
  APP_COMMUNICATION_CHAT,
  APP_COMM_ITEM,
  APP_COUPLE_MAIN,
  APP_GUILD,
  APP_HELPER_FAQ,
  APP_HELPER_HELP,
  APP_HELPER_TIP,
  APP_HOUSING,
  APP_INFO_NOTICE,
  APP_INVENTORY,
  APP_LOGOUT,
  APP_MAP,
  APP_MESSENGER_EX,
  APP_MOTION,
  APP_NAVIGATOR,
  APP_OPTIONEX,
  APP_PARTY,
  APP_PARTY_FIND,
  APP_PETFILTER,
  APP_QUEST_EX_LIST,
  APP_QUIT,
  APP_SKILL3,
  APP_STATUS1,
  APP_TELE16,
  APP_VENDOR_REVISION,
} from '../../gen/defs/resdata';
import {
  TID_APP_BAG_EX,
  TID_APP_CHARACTER,
  TID_APP_COMMITEM,
  TID_APP_COMMUNICATION,
  TID_APP_COMMUNICATION_CHAT,
  TID_APP_COMMUNITY,
  TID_APP_COMPANY,
  TID_APP_HELPER,
  TID_APP_HELPER_FAQ,
  TID_APP_HELPER_HELP,
  TID_APP_HELPER_TIP,
  TID_APP_INFOMATION,
  TID_APP_INFO_NOTICE,
  TID_APP_INVENTORY,
  TID_APP_ITEMTIME,
  TID_APP_LOGOUT,
  TID_APP_MAP,
  TID_APP_MESSENGER,
  TID_APP_MOTION,
  TID_APP_NAVIGATOR,
  TID_APP_OPTION,
  TID_APP_PARTY,
  TID_APP_QUEST,
  TID_APP_QUIT,
  TID_APP_SKILL,
  TID_APP_STATUS,
  TID_APP_VENDOR,
  TID_GAME_COUPLE,
  TID_GAME_HOUSING_BOX,
} from '../../gen/defs/text';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import { Menu } from '../controls/Menu';
import { SC } from '../dnd/shortcut';
import type { Rect } from '../layout/geometry';
import { rectWidth } from '../layout/geometry';
import type { BakeSpec } from '../skin/baker';
import { MISSING, themePath, type UiImageCache } from '../skin/images';
import { appletTooltipText } from '../tooltip/appletTooltip';
import type { AppletRegistry } from '../wm/applets';
import type { WndBase } from '../wnd/WndBase';
import type { WindowManager } from '../wm/WindowManager';

/** One entry of the start menu: an applet, or a folder with sub-entries. */
export type StartItem =
  | { readonly kind: 'applet'; readonly app: number; readonly label: string }
  | { readonly kind: 'folder'; readonly label: string; readonly items: readonly StartItem[] };

export interface StartMenuOptions {
  readonly features: Pick<ClientFeatureConfig, 'features'>['features'];
  readonly getText: (tid: number) => string;
  /** `authority >= GAMEMASTER` (the debug entry of the original is DROP). */
  readonly isGm: boolean;
  /** The applet exists in this build (built shell, feature on); everything else is hidden. */
  readonly exists: (app: number) => boolean;
}

/** `getText` with an English fallback for a missing string. */
function label(getText: (tid: number) => string, tid: number, en: string): string {
  const t = getText(tid);
  return t === '' || t === 'error' ? en : t;
}

/**
 * The items of the start menu (§11.4). `features.clean_start_menu` (default true) is the repack's short menu (Teleporter,
 * Party Finder and Pet Filter by their switches, "Basics >" with the main applets, Options, Logout, Quit); false is the
 * original's long menu with its Item / Communication / Community / Information / Helper folders. The Farm Bot and
 * F-Tool entries are absent while their features are reserved; the item mall and the GM debug window are DROP.
 */
export function buildStartMenu(o: StartMenuOptions): StartItem[] {
  const t = (tid: number, en: string): string => label(o.getText, tid, en);
  const app = (id: number, tid: number, en: string): StartItem => ({
    kind: 'applet',
    app: id,
    label: t(tid, en),
  });
  const prune = (items: StartItem[]): StartItem[] =>
    items
      .map((i) => (i.kind === 'folder' ? { ...i, items: prune([...i.items]) } : i))
      .filter((i) => (i.kind === 'folder' ? i.items.length > 0 : o.exists(i.app)));
  const folder = (labelText: string, items: StartItem[]): StartItem => ({
    kind: 'folder',
    label: labelText,
    items,
  });
  let items: StartItem[];
  if (o.features.clean_start_menu) {
    items = [];
    if (o.features.teleportation_system) {
      items.push({ kind: 'applet', app: APP_TELE16, label: 'Teleporter' });
    }
    if (o.features.party_find)
      items.push({ kind: 'applet', app: APP_PARTY_FIND, label: 'Party Finder' });
    if (o.features.pet_filter)
      items.push({ kind: 'applet', app: APP_PETFILTER, label: 'Pet Filter' });
    items.push(
      folder('Basics', [
        app(APP_BAG_EX, TID_APP_BAG_EX, 'Backpack'),
        app(APP_STATUS1, TID_APP_STATUS, 'Status'),
        app(APP_NAVIGATOR, TID_APP_NAVIGATOR, 'Navigator'),
        app(APP_CHARACTER3, TID_APP_CHARACTER, 'Character'),
        app(APP_INVENTORY, TID_APP_INVENTORY, 'Inventory'),
        app(APP_SKILL3, TID_APP_SKILL, 'Skill'),
        app(APP_MOTION, TID_APP_MOTION, 'Motion'),
        app(APP_MAP, TID_APP_MAP, 'Map'),
        app(APP_COMM_ITEM, TID_APP_ITEMTIME, 'Item Duration'),
        app(APP_PARTY, TID_APP_PARTY, 'Party'),
        app(APP_GUILD, TID_APP_COMPANY, 'Guild'),
        app(APP_COUPLE_MAIN, TID_GAME_COUPLE, 'Couple'),
        app(APP_MESSENGER_EX, TID_APP_MESSENGER, 'Messenger'),
        app(APP_COMMUNICATION_CHAT, TID_APP_COMMUNICATION_CHAT, 'Chat'),
      ]),
      app(APP_OPTIONEX, TID_APP_OPTION, 'Options'),
      app(APP_LOGOUT, TID_APP_LOGOUT, 'Logout'),
      app(APP_QUIT, TID_APP_QUIT, 'Quit'),
    );
  } else {
    items = [
      app(APP_STATUS1, TID_APP_STATUS, 'Status'),
      app(APP_NAVIGATOR, TID_APP_NAVIGATOR, 'Navigator'),
      app(APP_CHARACTER3, TID_APP_CHARACTER, 'Character'),
      app(APP_INVENTORY, TID_APP_INVENTORY, 'Inventory'),
      app(APP_BAG_EX, TID_APP_BAG_EX, 'Backpack'),
      app(APP_HOUSING, TID_GAME_HOUSING_BOX, 'Housing'),
      app(APP_VENDOR_REVISION, TID_APP_VENDOR, 'Private Shop'),
      app(APP_SKILL3, TID_APP_SKILL, 'Skill'),
      app(APP_QUEST_EX_LIST, TID_APP_QUEST, 'Quest'),
      app(APP_MOTION, TID_APP_MOTION, 'Motion'),
      app(APP_MAP, TID_APP_MAP, 'Map'),
      folder(t(TID_APP_COMMITEM, 'Item'), [app(APP_COMM_ITEM, TID_APP_ITEMTIME, 'Item Duration')]),
      folder(t(TID_APP_COMMUNICATION, 'Communication'), [
        app(APP_COMMUNICATION_CHAT, TID_APP_COMMUNICATION_CHAT, 'Chat'),
        app(APP_MESSENGER_EX, TID_APP_MESSENGER, 'Messenger'),
        app(APP_COUPLE_MAIN, TID_GAME_COUPLE, 'Couple'),
      ]),
      folder(t(TID_APP_COMMUNITY, 'Community'), [
        app(APP_PARTY, TID_APP_PARTY, 'Party'),
        app(APP_GUILD, TID_APP_COMPANY, 'Guild'),
      ]),
      folder(t(TID_APP_INFOMATION, 'Information'), [
        app(APP_INFO_NOTICE, TID_APP_INFO_NOTICE, 'Notice'),
      ]),
      folder(t(TID_APP_HELPER, 'Helper'), [
        app(APP_HELPER_HELP, TID_APP_HELPER_HELP, 'Help'),
        app(APP_HELPER_TIP, TID_APP_HELPER_TIP, 'Tip'),
        app(APP_HELPER_FAQ, TID_APP_HELPER_FAQ, 'FAQ'),
      ]),
      app(APP_OPTIONEX, TID_APP_OPTION, 'Options'),
      app(APP_LOGOUT, TID_APP_LOGOUT, 'Logout'),
      app(APP_QUIT, TID_APP_QUIT, 'Quit'),
    ];
  }
  return prune(items);
}

/** The y of the start menu: its bottom edge on the layout bottom (`rect.bottom - 48 - h`). */
export function startMenuY(layoutBottom: number, menuHeight: number): number {
  return layoutBottom - menuHeight;
}

/** The popup frame of the start menu: the `WndTaskMenu.tga` skin, items at `(10, 50 + 22 i)`. */
export class TaskMenu extends Menu {
  #texture = 'WndTaskMenu.tga';

  override buildBakeSpec(): BakeSpec | null {
    const r = this.rectWindowValue;
    const w = rectWidth(r);
    const h = r.b - r.t;
    if (w <= 0 || h <= 0) return null;
    return { width: w, height: h, ops: [{ op: 'image', texture: this.#texture, x: 0, y: 0 }] };
  }

  /** `SetWndRect(0, 0, texW, texH)` and the item rects of `OnInitialUpdate`. */
  layoutTask(texW: number, texH: number): void {
    this.items.forEach((it, i) => {
      it.setWndRect({ l: 10, t: 50 + i * 22, r: texW - 20, b: 50 + 20 + i * 22 });
    });
    this.setWndRect({ l: 0, t: 0, r: texW, b: texH });
  }
}

export interface StartMenuDeps {
  readonly wm: WindowManager;
  readonly applets: AppletRegistry;
  readonly images: UiImageCache;
  readonly getText: (tid: number) => string;
  readonly features: () => ClientFeatureConfig | undefined;
  readonly isGm: () => boolean;
  /** The window that receives the item commands (the taskbar). */
  readonly owner: () => WndBase | null;
  readonly executeApplet: (app: number) => void;
}

/** The controller of the start menu: builds the popup menus on the first open and positions them. */
export class StartMenu {
  readonly #d: StartMenuDeps;
  #menu: TaskMenu | null = null;
  #subs: Menu[] = [];
  #opening = false;

  constructor(deps: StartMenuDeps) {
    this.#d = deps;
  }

  /** The root popup (tests). */
  get root(): TaskMenu | null {
    return this.#menu;
  }

  /** `true` while the menu is shown. */
  isOpen(): boolean {
    return this.#menu?.isVisible() ?? false;
  }

  /** Hides the menu (a click on the taskbar, a layout change). */
  close(): void {
    this.#menu?.setVisibleAllMenu(false);
  }

  /** Destroys the popups (the taskbar was destroyed). */
  dispose(): void {
    for (const m of [this.#menu, ...this.#subs]) {
      if (m && !m.destroyed) this.#d.wm.destroy(m);
    }
    this.#menu = null;
    this.#subs = [];
  }

  /** `openStartMenu`: toggles; the menu is built and the skin size is known before it appears. */
  async open(): Promise<void> {
    if (this.isOpen()) {
      this.close();
      return;
    }
    if (this.#opening) return;
    const owner = this.#d.owner();
    if (!owner) return;
    this.#opening = true;
    try {
      if (!this.#menu || this.#menu.destroyed) await this.#build(owner);
      const menu = this.#menu;
      if (!menu) return;
      const layout = this.#d.wm.layoutRect;
      const h = menu.rectWindow.b - menu.rectWindow.t;
      menu.move({ x: 0, y: startMenuY(layout.b, h) });
      menu.setVisible(true);
      menu.setFocus();
    } finally {
      this.#opening = false;
    }
  }

  async #build(owner: WndBase): Promise<void> {
    const d = this.#d;
    const cfg = d.features();
    const items = buildStartMenu({
      features:
        cfg?.features ?? ({ clean_start_menu: true } as NonNullable<typeof cfg>['features']),
      getText: d.getText,
      isGm: d.isGm(),
      exists: (app) => d.applets.isAvailable(app),
    });
    const root = new TaskMenu();
    root.createMenu(owner);
    this.#menu = root;
    const add = (menu: Menu, item: StartItem): void => {
      if (item.kind === 'applet') {
        const b = menu.appendMenu(item.label, item.app);
        // a start-menu item can be dragged onto the taskbar (`SHORTCUT_APPLET`, §10.2)
        b.shortcut = { kind: SC.APPLET, id: item.app, type: 0, index: 0, userId: 0, data: 0 };
        const e = d.applets.get(item.app);
        if (e?.icon) b.setIcon(`icon/${e.icon}`);
        if (e) {
          const desc =
            e.tooltipTid !== null ? d.getText(e.tooltipTid) : (e.tooltipText ?? item.label);
          b.setToolTip(appletTooltipText(desc, d.applets.hotkeyOf(item.app), d.getText));
        }
        return;
      }
      const b = menu.appendMenu(item.label, 0);
      b.setIcon('icon/icon_folder.dds');
      const sub = new Menu();
      sub.createMenu(root);
      this.#subs.push(sub);
      for (const child of item.items) add(sub, child);
      b.setMenu(sub);
    };
    for (const item of items) add(root, item);
    const tex = await d.images.get(themePath('WndTaskMenu.tga'), 'nibble');
    const [w, h] = tex === MISSING ? [180, 50 + 10 + items.length * 22] : [tex.width, tex.height];
    root.layoutTask(w, h);
    root.requestBake();
  }
}

/** The rect of the start menu in the viewport (tests). */
export function startMenuRect(layoutBottom: number, w: number, h: number): Rect {
  return { l: 0, t: startMenuY(layoutBottom, h), r: w, b: startMenuY(layoutBottom, h) + h };
}
