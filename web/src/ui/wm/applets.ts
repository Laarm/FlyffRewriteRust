// The applet registry and the applet operations of the window manager (Plan/42 §12, ref/22 §3.4; WP-UI-15):
// `AppletRegistry` (the table of §12.1: APP id, keyword, icon, tooltip, hotkey letter, feature gate, and the factory
// that a window shell registers), `objectExecutorApplet` (§12.2 toggle rules), `openField` (§12.4) and the start-menu
// opener. The registry only knows about windows; what happens behind a window (ports) is 45's.

import {
  APP_BAG_EX,
  APP_BUFF_STATUS,
  APP_CHARACTER3,
  APP_COMMUNICATION_CHAT,
  APP_COMM_ITEM,
  APP_COUPLE_MAIN,
  APP_DIALOG_EX,
  APP_GH_FURNITURE_STORAGE,
  APP_GUILD,
  APP_HELPER_FAQ,
  APP_HELPER_HELP,
  APP_HELPER_TIP,
  APP_HOUSING,
  APP_INFO_NOTICE,
  APP_INVENTORY,
  APP_LOGOUT,
  APP_LORD_SKILL,
  APP_MAP,
  APP_MESSENGER_EX,
  APP_MODEL_VIEW,
  APP_MOTION,
  APP_NAVIGATOR,
  APP_OPTIONEX,
  APP_OPTION_MYINFO,
  APP_OPTION_SOUND,
  APP_OPTION_WINDOW,
  APP_PARTY,
  APP_PARTY_FIND,
  APP_PARTY_QUICK,
  APP_PETFILTER,
  APP_PIERCING,
  APP_QUEITMWARNING,
  APP_QUEST_EX_LIST,
  APP_QUIT,
  APP_SKILL3,
  APP_STATUS1,
  APP_SUMMON_ANGEL,
  APP_TELE16,
  APP_TEST,
  APP_TRADE,
  APP_VENDOR_REVISION,
} from '../../gen/defs/resdata';
import {
  TID_APP_BAG_EX,
  TID_APP_OPTION,
  TID_GAME_COUPLE,
  TID_GAME_HOUSING_BOX,
  TID_TIP_CHARACTER,
  TID_TIP_COMMUNICATION_CHAT,
  TID_TIP_DIALOG,
  TID_TIP_HELPER_FAQ,
  TID_TIP_HELPER_HELP,
  TID_TIP_HELPER_TIP,
  TID_TIP_INFOPANG,
  TID_TIP_INFO_NOTICE,
  TID_TIP_INVENTORY,
  TID_TIP_ITEMTIME,
  TID_TIP_LOGOUT,
  TID_TIP_MAP,
  TID_TIP_MESSENGER,
  TID_TIP_MOTION,
  TID_TIP_NAVIGATOR,
  TID_TIP_OPTION_MYINFO,
  TID_TIP_OPTION_SOUND,
  TID_TIP_OPTION_WINDOW,
  TID_TIP_PARTY,
  TID_TIP_COMPANY,
  TID_TIP_QUEST,
  TID_TIP_QUIT,
  TID_TIP_SKILL,
  TID_TIP_STATUS,
  TID_TIP_TRADE,
  TID_TIP_VENDOR,
} from '../../gen/defs/text';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import type { Logger } from '../../platform/log';
import type { WndNeuz } from '../wnd/WndNeuz';
import type { WindowManager } from './WindowManager';

/** The `[features]` switches that gate an applet (03 §4.2; the key of `ClientFeatureConfig`). */
export type AppletFeature = 'model_view' | 'pet_filter' | 'party_find' | 'teleportation_system';

/** One row of the applet table (§12.1). */
export interface AppletEntry {
  /** The `ResData.h` APP id: what crosses the wire inside taskbar shortcuts. */
  readonly app: number;
  /** The applet keyword of `/open` and `/close` (`m_pszKeyword`, ref/22 §3.4). */
  readonly keyword: string;
  /** The taskbar and start-menu icon (`Icon/...`), `null` for applets without one. */
  readonly icon: string | null;
  /** `TID_*` of the tooltip text, or `null` with {@link AppletEntry.tooltipText}. */
  readonly tooltipTid: number | null;
  /** The hard-coded English tooltip of the repack applets. */
  readonly tooltipText: string | null;
  /** The default letter hotkey (informational; 43's `APPLET_HOTKEYS` is the source of truth, PL-127). */
  readonly hotkey: string | null;
  /** The `[features]` switch that must be on for the applet to exist. */
  readonly feature: AppletFeature | null;
}

const row = (
  app: number,
  keyword: string,
  icon: string | null,
  tip: number | string | null,
  hotkey: string | null = null,
  feature: AppletFeature | null = null,
): AppletEntry => ({
  app,
  keyword,
  icon,
  tooltipTid: typeof tip === 'number' ? tip : null,
  tooltipText: typeof tip === 'string' ? tip : null,
  hotkey,
  feature,
});

/**
 * The applets of §12.1 in their registration order (`CWndMgr::AddAllApplet`, FuncApplet.cpp:162-308). The `APP_FTOOL`,
 * `APP_REFRESHER`, `APP_WEBBOX*`, `APP_DEBUGINFO`, `APP_INFOPANG`, `APP_2ND_PASSWORD_NUMBERPAD` and `APP_GUILD_VOTE` rows are
 * DROP/DEFER (never registered); `APP_WORLD` is not a window here.
 */
export const APPLET_TABLE: readonly AppletEntry[] = [
  row(APP_NAVIGATOR, 'WndNavigator', 'Icon_Navigator.dds', TID_TIP_NAVIGATOR, 'N'),
  row(APP_STATUS1, 'WndStatus', 'Icon_Status.dds', TID_TIP_STATUS),
  row(APP_MODEL_VIEW, 'WndModelView', 'Icon_Applet.dds', 'Model Viewer', null, 'model_view'),
  row(APP_MAP, 'WndMap', 'Icon_Applet.dds', TID_TIP_MAP, 'M'),
  row(APP_LORD_SKILL, 'WndLordSkill', 'Icon_Infopang.dds', TID_TIP_INFOPANG, 'L'),
  row(APP_PETFILTER, 'WndPetfilter', 'Icon_Applet.dds', 'Pet Filter', 'Y', 'pet_filter'),
  row(APP_PARTY_FIND, 'WndPartyFind', 'Icon_Applet.dds', 'Party Finder', 'U', 'party_find'),
  row(APP_TELE16, 'WndTeleporter', 'Icon_Motion.dds', 'Teleporter', 'V', 'teleportation_system'),
  row(APP_HOUSING, 'WndHousing', 'Icon_Housing.dds', TID_GAME_HOUSING_BOX),
  row(APP_GH_FURNITURE_STORAGE, 'WndGuildHousing', 'Icon_Housing.dds', TID_GAME_HOUSING_BOX),
  row(APP_CHARACTER3, 'WndCharacter', 'Icon_Character.dds', TID_TIP_CHARACTER, 'H'),
  row(APP_INVENTORY, 'WndInventory', 'Icon_Inventory.dds', TID_TIP_INVENTORY, 'I'),
  row(APP_SKILL3, 'WndSkill', 'Icon_Skill.dds', TID_TIP_SKILL, 'K'),
  row(APP_MOTION, 'WndMotion', 'Icon_Motion.dds', TID_TIP_MOTION, 'O'),
  row(APP_TRADE, 'WndTrade', 'Icon_Trade.dds', TID_TIP_TRADE),
  row(APP_VENDOR_REVISION, 'WndVendor', 'Icon_Applet.dds', TID_TIP_VENDOR),
  row(APP_QUEST_EX_LIST, 'WndQuest', 'Icon_Quest.dds', TID_TIP_QUEST),
  row(APP_PARTY, 'WndParty', 'Icon_Troupe.dds', TID_TIP_PARTY, 'P'),
  row(APP_GUILD, 'WndGuild', 'Icon_Troupe.dds', TID_TIP_COMPANY, 'G'),
  row(APP_BAG_EX, 'WndBagEx', 'Icon_Inventory.dds', TID_APP_BAG_EX, 'B'),
  row(APP_COMM_ITEM, 'WndCommItem', 'Icon_CItemTime.dds', TID_TIP_ITEMTIME, 'J'),
  row(APP_TEST, 'WndUpgradeBase', 'Icon_Troupe.dds', TID_TIP_PARTY),
  row(APP_PIERCING, 'WndPiercing', 'Icon_Troupe.dds', TID_TIP_PARTY),
  row(APP_COMMUNICATION_CHAT, 'WndChat', 'Icon_Chat.dds', TID_TIP_COMMUNICATION_CHAT),
  row(APP_MESSENGER_EX, 'WndMessenger', 'Icon_Messenger.dds', TID_TIP_MESSENGER, 'E'),
  row(APP_OPTION_SOUND, 'WndOptSound', 'Icon_OptSound.dds', TID_TIP_OPTION_SOUND),
  row(APP_OPTION_WINDOW, 'WndOptWindow', 'Icon_Applet.dds', TID_TIP_OPTION_WINDOW),
  row(APP_OPTION_MYINFO, 'WndOptMyInfo', 'Icon_Applet.dds', TID_TIP_OPTION_MYINFO),
  row(APP_OPTIONEX, 'WndOption', 'Icon_Applet.dds', TID_APP_OPTION),
  row(APP_INFO_NOTICE, 'WndInfoNotice', 'Icon_Applet.dds', TID_TIP_INFO_NOTICE),
  row(APP_HELPER_HELP, 'WndHelp', 'Icon_Helper.dds', TID_TIP_HELPER_HELP),
  row(APP_HELPER_TIP, 'WndHelpTip', 'Icon_Helper.dds', TID_TIP_HELPER_TIP),
  row(APP_HELPER_FAQ, 'WndHelpFAQ', 'Icon_Helper.dds', TID_TIP_HELPER_FAQ),
  row(APP_LOGOUT, 'WndLogout', 'Icon_Logout.dds', TID_TIP_LOGOUT),
  row(APP_QUIT, 'WndQuit', 'Icon_Quit.dds', TID_TIP_QUIT),
  row(APP_PARTY_QUICK, 'WndPartyQuick', null, TID_TIP_DIALOG),
  row(APP_BUFF_STATUS, 'WndBuffStatus', null, TID_TIP_DIALOG),
  row(APP_COUPLE_MAIN, 'WndCoupleManager', 'Icon_Couple.dds', TID_GAME_COUPLE, 'F'),
];

/** What `register` needs: the window class factory of an applet of the table (or a new row). */
export interface AppletRegistration {
  readonly app: number;
  readonly factory: () => WndNeuz;
  /** A row for an applet that is not in {@link APPLET_TABLE}. */
  readonly entry?: AppletEntry;
}

export interface AppletRegistryOptions {
  /** `ClientFeatureConfig` (read lazily: the bootstrap answer may arrive after the UI was created). */
  readonly features?: () => ClientFeatureConfig | undefined;
  readonly log?: Logger;
  /** The table to start from (the tests pass a smaller one); {@link APPLET_TABLE} by default. */
  readonly table?: readonly AppletEntry[];
}

/** `AddAppletFunc`: the applets of the table with the factories the window shells registered. */
export class AppletRegistry {
  readonly #entries = new Map<number, AppletEntry>();
  readonly #factories = new Map<number, () => WndNeuz>();
  readonly #features: () => ClientFeatureConfig | undefined;
  readonly #log: Logger | undefined;
  readonly #warned = new Set<number>();
  readonly #wm: WindowManager | null;
  #hotkeys: ReadonlyMap<number, string> = new Map();

  constructor(wm: WindowManager | null, options: AppletRegistryOptions = {}) {
    this.#wm = wm;
    this.#features = options.features ?? (() => undefined);
    this.#log = options.log;
    for (const e of options.table ?? APPLET_TABLE) this.#entries.set(e.app, e);
  }

  /**
   * Registers the factory of a window shell (`AddAppletFunc(createfunc, ...)`): the window manager's `open(app)` uses
   * it from now on and the applet leaves the "pending" state.
   */
  register(r: AppletRegistration): void {
    if (r.entry) this.#entries.set(r.app, r.entry);
    this.#factories.set(r.app, r.factory);
    this.#wm?.registerFactory(r.app, r.factory);
  }

  /** The row of an applet. */
  get(app: number): AppletEntry | undefined {
    return this.#entries.get(app);
  }

  /** The row by `/open` keyword, ASCII case-insensitive (ref/22 §3.4 `m_pszKeyword`). */
  byKeyword(keyword: string): AppletEntry | undefined {
    const k = keyword.toLowerCase();
    for (const e of this.#entries.values()) {
      if (e.keyword.toLowerCase() === k) return e;
    }
    return undefined;
  }

  /** Every row in registration order. */
  list(): readonly AppletEntry[] {
    return [...this.#entries.values()];
  }

  /** `true` when a shell registered a factory (the applet is "built" in this milestone). */
  isBuilt(app: number): boolean {
    return this.#factories.has(app);
  }

  /** `true` when the row's `[features]` switch is on (a row without a switch is always on). */
  featureOn(app: number): boolean {
    const f = this.#entries.get(app)?.feature;
    if (!f) return true;
    const cfg = this.#features();
    if (!cfg) return true;
    return cfg.features[f];
  }

  /** The applet exists in this build and milestone: switched on and built. */
  isAvailable(app: number): boolean {
    return this.isBuilt(app) && this.featureOn(app);
  }

  /** Logs once that an applet is pending (dev builds) and returns whether it is. */
  isPending(app: number): boolean {
    if (this.isBuilt(app)) return false;
    if (!this.#warned.has(app)) {
      this.#warned.add(app);
      this.#log?.warn('ui', `applet ${String(app)} is pending (no window shell registered yet)`);
    }
    return true;
  }

  /** The hotkey letters of 43's `APPLET_HOTKEYS` (app -> letter), used by the tooltips; replaces the defaults. */
  setHotkeys(map: ReadonlyMap<number, string>): void {
    this.#hotkeys = map;
  }

  /** The letter shown in the tooltip of an applet: 43's table when set, else the §12.1 default. */
  hotkeyOf(app: number): string | null {
    return this.#hotkeys.get(app) ?? this.#entries.get(app)?.hotkey ?? null;
  }
}

// ---- §12.2 `objectExecutorApplet` ----------------------------------------------------------------------------

/** The collaborators of the toggle rules that live outside the window manager. */
export interface AppletHooks {
  /** `APP_LOGOUT`: the custom box of §14.2 (refused in attack mode by the shell: `TID_PK_COMBAT_END`, 45). */
  openLogout?(): void;
  /** `APP_QUIT`: the custom box of §14.2. */
  openQuit?(): void;
  /** The own party has at least two members (the quick window is only created then). */
  partyHasQuick?(): boolean;
  /** `APP_MAP` toggles the world-map overlay (45), not a window. */
  toggleWorldMap?(): void;
  /** `wm.openStartMenu`: opens the start menu (§11.4). */
  openStartMenu?(): void;
  /** `openField`: creates the taskbar. */
  createTaskbar?(): void;
  /** `openField`: clears the chat log. */
  clearChat?(): void;
}

/**
 * `wm.objectExecutorApplet(id)` (`ObjectExecutor(SHORTCUT_APPLET)`, WndManager.cpp:2626-2780, §12.2):
 *
 * 1. `APP_DIALOG_EX` open: nothing;
 * 2. `APP_SKILL3`: refused while `APP_QUEITMWARNING` is open; open with unspent point edits: the re-skill warning
 *    instead of closing;
 * 3. `APP_QUIT` / `APP_LOGOUT`: the custom boxes;
 * 4. `APP_PARTY` toggles `APP_PARTY` and `APP_PARTY_QUICK` (quick only with at least 2 members);
 * 5. `APP_MAP`: the world map toggle;
 * 6. `APP_VENDOR_REVISION`: refused while `APP_SUMMON_ANGEL` is open;
 * 7. otherwise a toggle: open -> destroy and focus the world, closed -> open.
 *
 * An applet without a registered shell does nothing (dev builds log once); an applet whose feature switch is off does
 * not exist.
 */
export function objectExecutorApplet(
  wm: WindowManager,
  registry: AppletRegistry,
  hooks: AppletHooks,
  app: number,
): void {
  if (wm.isOpen(APP_DIALOG_EX)) return;
  if (app === APP_SKILL3) {
    if (wm.isOpen(APP_QUEITMWARNING)) return;
    if (wm.isOpen(APP_SKILL3) && wm.skillTreeEsc?.hasUnspentEdits()) {
      wm.skillTreeEsc.openWarning();
      return;
    }
  }
  if (app === APP_QUIT) {
    hooks.openQuit?.();
    return;
  }
  if (app === APP_LOGOUT) {
    hooks.openLogout?.();
    return;
  }
  if (app === APP_MAP) {
    hooks.toggleWorldMap?.();
    return;
  }
  if (app === APP_VENDOR_REVISION && wm.isOpen(APP_SUMMON_ANGEL)) return;
  if (!registry.featureOn(app)) return;
  if (app === APP_PARTY) {
    const wasOpen = wm.isOpen(APP_PARTY);
    toggle(wm, registry, APP_PARTY);
    if (wasOpen) {
      wm.close(APP_PARTY_QUICK);
    } else if (wm.isOpen(APP_PARTY) && (hooks.partyHasQuick?.() ?? false)) {
      if (!wm.isOpen(APP_PARTY_QUICK)) openApp(wm, registry, APP_PARTY_QUICK);
    }
    return;
  }
  toggle(wm, registry, app);
}

function toggle(wm: WindowManager, registry: AppletRegistry, app: number): void {
  if (wm.isOpen(app)) {
    wm.close(app);
    wm.root.setFocus();
    return;
  }
  openApp(wm, registry, app);
}

function openApp(wm: WindowManager, registry: AppletRegistry, app: number): void {
  if (registry.isPending(app)) return;
  wm.open(app);
}

/** `openApplet(app)`: open or focus (`CreateApplet`), never toggles. Pending applets do nothing. */
export function openApplet(wm: WindowManager, registry: AppletRegistry, app: number): void {
  if (!registry.featureOn(app)) return;
  openApp(wm, registry, app);
}

/**
 * `wm.openField()` (`OpenField`, WndManager.cpp:2225-2240, §12.4): the taskbar, a cleared chat log, then the chat
 * window, the navigator and the status window (each only when built in the current milestone). Windows that were
 * open at logout are NOT reopened (the original's code is commented out, KEEP).
 */
export function openField(wm: WindowManager, registry: AppletRegistry, hooks: AppletHooks): void {
  hooks.createTaskbar?.();
  hooks.clearChat?.();
  for (const app of [APP_COMMUNICATION_CHAT, APP_NAVIGATOR, APP_STATUS1]) {
    if (registry.isBuilt(app) && registry.featureOn(app)) wm.open(app);
  }
}
