// Public API of the UI module (Plan/42 §2.1): `createUi(host)` builds the layout registry, the image cache, the skin
// baker, the atlases and the window manager of one UI instance. The window-side classes are exported for the
// window shells (`WndNeuz`, `Control`), the React glue (`UiRoot`, `FrameWindowView`) and the dev surfaces.
// WP-UI-05..09 add the controls, tooltips, message boxes, cursors, scaling and persistence to `UiApi`.

import { createLogger } from '../platform/log';
import type { UiApi as UiPort } from '../game/ports';
import { WINDOW_CATALOG } from './catalog';
import type { UiHost } from './host';
import { LayoutRegistry } from './layout/registry';
import { UiAtlases } from './skin/atlas';
import { SkinBaker } from './skin/baker';
import { createBrowserDecoder, UiImageCache } from './skin/images';
import { ChatService } from './chat';
import { CursorManager } from './cursor/CursorManager';
import { MessageBoxService, type MessageBoxApi } from './msgbox/MessageBoxService';
import { UiOptionsStore } from './options';
import { SessionService } from './session';
import { AppearanceWindowsService } from './windows/appearance';
import { BankWindowsService } from './windows/bank';
import { MailWindowsService } from './windows/mail';
import { PocketWindowsService } from './windows/pockets';
import { CharService } from './windows/char';
import { ItemDialogsService } from './windows/dialogs';
import { DuelWindowsService } from './windows/duel';
import { ExchangeWindowsService } from './windows/exchange';
import { GuildWindowsService } from './windows/guild';
import { ItemUseWindowsService } from './windows/itemuse';
import { JobWindowsService } from './windows/jobs';
import { MessengerWindowsService } from './windows/messenger';
import { CoupleWindowsService } from './windows/couple';
import { NavWindowsService } from './windows/nav';
import { NpcWindowsService } from './windows/npc';
import { PartyWindowsService } from './windows/party';
import { PetWindowsService } from './windows/pets';
import { QuestWindowsService } from './windows/quest';
// ---- lord (ui-lord, M7)
import { Gc1to1WindowsService } from './windows/gc1to1';
import { LordWindowsService } from './windows/lord';
// ---- housing / guild house (ui-house-quiz)
import { HousingWindowsService } from './windows/housing';
import { QuizWindowsService } from './windows/quiz';
// ---- secret room and tax (ui-secret-tax, M7)
import { SecretRoomWindowsService } from './windows/secretroom';
// ---- rainbow race and quiz (ui-rainbow)
import { RainbowWindowsService } from './windows/rainbow';
// ---- mini-games (ui-minigames, M7)
import { MiniGameWindowsService } from './windows/minigame';
import { groupThousands } from './text/format';
import { SiegeWindowsService } from './windows/siege';
import { SkillWindowsService } from './windows/skills';
import { ReadableWindowsService } from './windows/readable';
import { TradeWindowsService } from './windows/trade';
import { VendorWindowsService } from './windows/vendor';
import { UltimateWindowsService } from './windows/ultimate';
import { UpgradeWindowsService } from './windows/upgrade';
import { createUiProbe, type UiProbe, type UiProbeOptions } from './probe';
import { type CursorImageIo, UiRamp } from './skin/ramp';
import { Taskbar } from './taskbar/Taskbar';
import { TooltipService } from './tooltip/TooltipService';
import type { AppletRegistry } from './wm/applets';
import { RegInfoStore } from './wm/regInfo';
import { UiScaler } from './wm/scale';
import { WindowManager } from './wm/WindowManager';
import { lazyWindowsReady, preloadLazyWindows } from './wnd/lazy';

export type { UiHost } from './host';
export { WINDOW_CATALOG, BUILT_CONTROLLERS, catalogCounts, catalogStatus } from './catalog';
export type { CatalogEntry, Milestone, Priority } from './catalog';
export { LayoutRegistry } from './layout/registry';
export type { AppKey, AppletLayout, ControlLayout, WidcKey } from './layout/registry';
export * from './layout/geometry';
export * from './layout/styles';
export { UiImageCache, MISSING, devAssetUrls, uiAssetUrlsFromManifest } from './skin/images';
export type { UiAssetUrls, ImageDecoder, KeyMode } from './skin/images';
export { SkinBaker } from './skin/baker';
export { UiAtlases } from './skin/atlas';
export { WindowManager } from './wm/WindowManager';
export type { TaskbarApi, WindowManagerApi, WindowManagerCore } from './wm/api';
export { APPLET_TABLE, AppletRegistry } from './wm/applets';
export type { AppletEntry, AppletHooks, AppletRegistration } from './wm/applets';
export { ChatService } from './chat';
export { SessionService } from './session';
export { AppearanceWindowsService } from './windows/appearance';
export { BankWindowsService } from './windows/bank';
export { MailWindowsService } from './windows/mail';
export { PocketWindowsService } from './windows/pockets';
export type {
  DeleteConfirmRequest,
  MailDraft,
  MailRow,
  MailSendForm,
  MailView,
  PostActions,
  PostRequest,
  ReadActions,
  ReadRequest,
  RequestingBoxRequest,
  SendPorts,
} from './windows/mail/ports';
export type {
  BankActions,
  BankCompartmentView,
  BankPasswordRequest,
  BankRequest,
  BankView,
  ConfirmBankRequest,
  PinClose,
} from './windows/bank/ports';
export type {
  PocketActions,
  PocketPanelView,
  PocketPorts,
  PocketView,
} from './windows/pockets/ports';
export type {
  ChangeFaceRequest,
  ChangeSexPreview,
  ChangeSexRequest,
  ItemTransyRequest,
  TransyPair,
} from './windows/appearance/ports';
export { CharService, NULL_CORE_PORTS } from './windows/char';
export { ItemDialogsService } from './windows/dialogs';
export { DuelWindowsService } from './windows/duel';
export type {
  DuelConfirmRequest,
  DuelResultRequest,
  ResurrectionRequest,
} from './windows/duel/ports';
export { ItemUseWindowsService } from './windows/itemuse';
export { JobWindowsService } from './windows/jobs';
export type { ChangeClassRequest, FastJobRequest } from './windows/jobs/ports';
export { SkillWindowsService } from './windows/skills';
export { UltimateWindowsService } from './windows/ultimate';
export type {
  ChangeWeaponRequest,
  ExtractionRequest,
  JewelLabel,
  MixJewelRequest,
  RemoveJewelRequest,
  SmeltJewelRequest,
  UltimateItem,
  WeaponFacts,
} from './windows/ultimate/ports';
export { UpgradeWindowsService } from './windows/upgrade';
export type * from './windows/upgrade/ports';
export { ExchangeWindowsService } from './windows/exchange';
export type { ExchangeRequest } from './windows/exchange/ports';
export type {
  HeroSkillUpRequest,
  ReSkillControlRequest,
  SkillTreeActions,
  SkillTreeView,
} from './windows/skills/ports';
export { NpcWindowsService } from './windows/npc';
export { NavWindowsService, WorldMapLayer } from './windows/nav';
export type {
  CollectingActions,
  CollectingPorts,
  CollectingView,
  InspectItem,
  NavFilterView,
  NavHover,
  NavigatorActions,
  NavigatorPorts,
  NavigatorView,
  NavNpcEntry,
  NavSize,
  QueryEquipPorts,
  QueryEquipView,
  Tele16Actions,
  Tele16Boss,
  Tele16Drop,
  Tele16Entry,
  Tele16Ports,
  Tele16View,
  WorldMapArrow,
  WorldMapButton,
  WorldMapIcon,
  WorldMapPorts,
  WorldMapView,
} from './windows/nav/ports';
export type * from './windows/npc/ports';
export { PartyWindowsService } from './windows/party';
export { MessengerWindowsService } from './windows/messenger';
export { GuildWindowsService } from './windows/guild';
export { SiegeWindowsService } from './windows/siege';
export type {
  BoardRequest,
  CancelRequest,
  InfoBox2Request,
  InfoBoxRequest,
  JoinRequest,
  OfferRequest,
  RankingRequest,
  RankPersonRequest,
  RankRow,
  RankView,
  ResultRequest,
  SelectionMember,
  SelectionRequest,
  SelStartRequest,
  SiegeHost,
  StateRequest,
  WarStateRequest,
} from './windows/siege/ports';
export type {
  BankLogRequest,
  BankLogView,
  DismissRequest,
  GuildActions,
  GuildBankActions,
  GuildBankRequest,
  GuildBankView,
  GuildMemberRow,
  GuildMenuEntry,
  GuildPorts,
  GuildView,
  GuildWarView,
  InviteRequest,
  LogoRequest,
  MeritRequest,
  NameRequest,
  NicknameRequest,
  NoticeRequest,
  PayRequest,
  PowerView,
  WarDeclRequest,
  WarRequestRequest,
  WarYesNoRequest,
} from './windows/guild/ports';
export { CoupleWindowsService } from './windows/couple';
export type {
  CoupleActions,
  CoupleInfoView,
  CoupleMessageRequest,
  CouplePorts,
  CoupleSkillCell,
  CoupleSkillView,
  HonorActions,
  HonorEntry,
  HonorPorts,
  HonorView,
} from './windows/couple/ports';
export { PetWindowsService } from './windows/pets';
export type * from './windows/pets/ports';
export { QuestWindowsService } from './windows/quest';
export type * from './windows/quest/ports';
// ---- lord (ui-lord, M7)
export { LordWindowsService } from './windows/lord';
export type * from './windows/lord/ports';
export { Gc1to1WindowsService } from './windows/gc1to1';
// ---- housing / guild house (ui-house-quiz)
export { HousingWindowsService } from './windows/housing';
export type * from './windows/housing/ports';
export { QuizWindowsService } from './windows/quiz';
export type * from './windows/quiz/ports';
// ---- secret room and tax (ui-secret-tax, M7)
export { SecretRoomWindowsService } from './windows/secretroom';
export type * from './windows/secretroom/ports';
// ---- rainbow race and quiz (ui-rainbow)
export { RainbowWindowsService } from './windows/rainbow';
export type * from './windows/rainbow/ports';
// ---- mini-games (ui-minigames, M7)
export { MiniGameWindowsService } from './windows/minigame';
export type * from './windows/minigame/ports';
export { TradeWindowsService } from './windows/trade';
export { VendorWindowsService } from './windows/vendor';
export type {
  VendorActions,
  VendorBuyRequest,
  VendorChatActions,
  VendorChatPorts,
  VendorChatView,
  VendorEntry,
  VendorPorts,
  VendorSellRequest,
  VendorView,
} from './windows/vendor/ports';
export { ReadableWindowsService } from './windows/readable';
export type { ReadableRequest } from './windows/readable/ports';
export type {
  TradeActions,
  TradeCell,
  TradeFinalConfirm,
  TradePorts,
  TradeRequestWindow,
  TradeSideView,
  TradeView,
} from './windows/trade/ports';
export type {
  FinderActions,
  FinderPorts,
  FinderRow,
  FinderView,
  PartyActions,
  PartyChangeNameRequest,
  PartyChangeTroupRequest,
  PartyConfirmRequest,
  PartyLeaveRequest,
  PartyMemberRow,
  PartyPorts,
  PartySkillRow,
  PartyView,
  SummonPartyMsgRequest,
  SummonPartyRequest,
  SummonPartyUseRequest,
} from './windows/party/ports';
export type {
  DialogItem,
  DropConfirmRequest,
  DropItemRequest,
  InvenRemoveRequest,
  QueItemWarningRequest,
  RevivalActions,
  RevivalChoice,
  RevivalRequest,
  RevivalView,
} from './windows/dialogs/ports';
export type * from './windows/char/ports';
export { Taskbar, NULL_TASKBAR_PORTS } from './taskbar/Taskbar';
export type { TaskbarPorts, TaskbarActions, SlotIcon } from './taskbar/TaskbarWnd';
export { chooseTaskbarLayout, TASKBAR_LAYOUTS } from './taskbar/layout';
export { TaskbarModel } from './taskbar/model';
export { buildStartMenu } from './taskbar/StartMenu';
export type { WindowRectInfo, WmSnapshot, PointerResult } from './wm/WindowManager';
export { WndBase } from './wnd/WndBase';
export type { PointerInfo } from './wnd/WndBase';
export { WndNeuz } from './wnd/WndNeuz';
export type { FrameState } from './wnd/WndNeuz';
export { Control } from './wnd/Control';
export type { ControlViewProps, ControlViewState } from './wnd/Control';
export { LayoutWindow } from './wnd/LayoutWindow';
export { FrameWindowView } from './wnd/FrameWindowView';
export { UiRoot } from './UiRoot';
export { TooltipService } from './tooltip/TooltipService';
export { itemHover } from './tooltip/itemHover';
export { abilityValue, createRulesNumbersProvider, upgradeBonus } from './tooltip/numbers';
export type {
  ItemNumbersInput,
  RulesNumbersOptions,
  RulesNumbersSource,
  WearerInput,
} from './tooltip/numbers';
export type { ItemHoverOptions } from './tooltip/itemHover';
export type {
  ItemNumbersProvider,
  ItemTooltipCtx,
  ItemTooltipNumbers,
  ItemView,
} from './tooltip/types';
export { DragController, FORBID_MS } from './dnd/DragController';
export { SC } from './dnd/shortcut';
export type { Shortcut } from './dnd/shortcut';
export { ItemGrid, CELL, CELL_HIT } from './controls/ItemGrid';
export type { ItemGridDelegate } from './controls/ItemGrid';
export { IconCell } from './icons/IconCell';
export type { IconCellData } from './icons/IconCell';
export { CursorManager } from './cursor/CursorManager';
export { CUR, CURSORS } from './cursor/cursors';
export { MessageBoxService } from './msgbox/MessageBoxService';
export type { MessageBoxApi } from './msgbox/MessageBoxService';
export { MB, MBID } from './msgbox/types';
export { UiOptionsStore, normalizeUiOptions, DEFAULT_UI_OPTIONS } from './options';
export type { UiOptions, UiOptionsSource } from './options';
export { UiScaler, autoUiScale, resolveUiScale, virtualViewport, TASKBAR_HEIGHT } from './wm/scale';
export type { UiScaleState, UiMode } from './wm/scale';
export { RegInfoStore, defaultIsApplet, REGINFO_KEY_PREFIX } from './wm/regInfo';
export { UiRamp, RAMP_FILTER_ID, rampPixels, rampTableValues } from './skin/ramp';
export { createUiProbe } from './probe';
export type { UiProbe, UiProbeOptions, ProbeAction } from './probe';

/** What `createUi` returns: the port the game loop calls plus the pieces of the framework. */
export interface UiApi extends UiPort {
  readonly host: UiHost;
  readonly wm: WindowManager;
  readonly registry: LayoutRegistry;
  readonly images: UiImageCache;
  readonly baker: SkinBaker;
  readonly atlases: UiAtlases;
  readonly catalog: typeof WINDOW_CATALOG;
  /** The tooltip host (WP-UI-08): `put`, `putItem`, `putSkill`, `registerComposers`, ... */
  readonly tooltip: TooltipService;
  /** The standard dialogs (WP-UI-08): message boxes, the quantity box, the confirm box. */
  readonly msgbox: MessageBoxApi;
  /** The cursor registry (WP-UI-08): `setWorldCursor(id)` for 43. */
  readonly cursor: CursorManager;
  /** The applet table and the window factories (WP-UI-15): `register`, `get`, `byKeyword`. */
  readonly applets: AppletRegistry;
  /** The taskbar service (WP-UI-14): model, start menu, `TaskbarApi` for 43. */
  readonly taskbar: Taskbar;
  /** The chat router, ring, window shells and ports (WP-UI-16): `putString`, `setPorts`, `setOverlays`, `focusInput`. */
  readonly chat: ChatService;
  /** The Logout / Quit boxes and the logout countdown window (WP-UI-17): `setActions`, `openCountdown`. */
  readonly session: SessionService;
  /** The death and item-handling dialogs (WP-UI-22): revival, drop quantity / confirm, trash delete, quest item warning. */
  readonly dialogs: ItemDialogsService;
  /** The duel request, the duel result and the resurrection offer (WP-UI-41): `openConfirm`, `openResult`, `openResurrection`. */
  readonly duel: DuelWindowsService;
  /** The item-use dialogs and the buff windows (WP-UI-28): equip-bind confirm, quest item info, return-scroll village, item duration list, angel summon, the buff strip and status window. */
  readonly itemUse: ItemUseWindowsService;
  /** The appearance windows (WP-UI-35): the face window, the sex-change window, the item-transy window; `openChangeFace`, `openChangeSex`, `openItemTransy`. */
  readonly appearance: AppearanceWindowsService;
  /** The bank (WP-UI-25): the three-compartment bank window, the PIN entry and the PIN window; `openBank`, `openConfirm`, `openPassword`, `closeAll`. */
  readonly bank: BankWindowsService;
  /** The pocket window (WP-UI-25): the backpack applet `APP_BAG_EX` (hotkey B) with the basic pocket and the two extra bags; `setPorts`, `open`. */
  readonly pockets: PocketWindowsService;
  /** The mail windows (WP-UI-27): the post window with the send tab and the inbox, the reader, the delete confirmation and the requesting box; `openPost`, `openRead`, `mailboxArrived`, `closeAll`. */
  readonly mail: MailWindowsService;
  /** The ultimate-weapon windows (WP-UI-32, WP-UI-33): mixing, extraction, jewel setting, weapon change, jewel removal; `openMixJewel` ... `openRemoveJewel`. */
  readonly ultimate: UltimateWindowsService;
  /** The upgrade, element, awakening and piercing windows (WP-UI-29, -30, -31): the safe upgrade window, element removal and stones, awakening, blessing / level-down / pet awakening cancel, piercing; `openSmeltSafety` ... `openRemovePiercing`. */
  readonly upgrade: UpgradeWindowsService;
  /** The item-exchange list of an NPC (WP-UI-36 shell, WP-CFEAT-26): `APP_DIALOG_EVENT`; `open`, `window`. */
  readonly exchange: ExchangeWindowsService;
  /** The skill tree and its helper windows (WP-UI-38): `APP_SKILL3`, the hero skill-up, the skill distribution; `setPorts`, `openTree`. */
  readonly skills: SkillWindowsService;
  /** The fast job change and the class-change scroll windows (WP-UI-38): `openFastJobChange`, `openChangeClass`. */
  readonly jobs: JobWindowsService;
  /** The party windows (WP-UI-39, -40): the party window with its tabs, the simple display, the finder, the invitation and the summon windows. */
  readonly party: PartyWindowsService;
  /** The messenger windows (WP-UI-43, -44): the messenger with its friend, guild and campus pages, the IM windows, the popups, the friend-request, note, ignore-list and summon dialogs; `setPorts`, `openMessage`, `openInstant` ... */
  readonly messenger: MessengerWindowsService;
  /** The guild windows (WP-UI-45..48): the guild window with its five pages, the invitation, name, nickname, disband, notice, logo, salary and contribution dialogs, the guild bank with its log and the guild-war dialogs; `setPorts`, `openInvite` ... `openWarRequest`. */
  readonly guild: GuildWindowsService;
  /** The guild-siege windows (WP-UI-49..51): the bid window, the application list, the line-up editor, the gate choice, the boards, the info boxes, the cancel and join confirmations, the result window and the person ranking; `openOffer` ... `openRankPerson`. */
  readonly siege: SiegeWindowsService;
  /** The couple windows (WP-UI-52): the couple window with its information and skill tabs and the couple message window; `setPorts`, `openMessage`. The honor page is a tab of `ui.char`. */
  readonly couple: CoupleWindowsService;
  /** The pet windows (WP-UI-33, -34): the pet status, the food count and the food mill, the chance scroll, the egg transformer, the pet exchange, the buff pet sockets, the pet filter applet; `openPetStatus` ... `setFilterPort`. */
  readonly pets: PetWindowsService;
  /** The quest windows (WP-UI-53): the quest list applet (registered by `enableApplet` only when quest data is loaded, PL-148), its detail, the quick-info tracker and the Heaven Tower; `openDetail`, `openQuickInfo`, `openHeavenTower`. */
  readonly quest: QuestWindowsService;
  // ---- lord (ui-lord, M7)
  /** The lord windows (WP-UI-54, -55): the election (state, tender, pledge, confirm, vote), the lord's event window, the lord skill applet with its name prompt and the two information windows; `setPorts`, `openState` ... `openRPInfo`. */
  readonly lord: LordWindowsService;
  /** The 1:1 guild combat application status (WP-UI-55, the 676 shell only; the feature is `features/events/gc1to1`): `openOfferState`, `offerState`, `closeAll`. */
  readonly gc1to1: Gc1to1WindowsService;
  // ---- housing / guild house (ui-house-quiz)
  /** The housing and guild-house windows (WP-UI-58, WP-UI-59): the furniture list applet, the room list, the quit box, the guild-house menu, the storage applet, the SHOWONE strip, the upkeep box and the teleporter shell; `setHousingPort`, `setStoragePort`, `openRoomList` ... `refresh`. */
  readonly housing: HousingWindowsService;
  /** The quiz window shells (WP-UI-59; the feature is ui-rainbow's): `openButton`, `openConfirm`, `openQuestionOx`, `openQuestion4c`, `update`, `closeQuestions`. */
  readonly quiz: QuizWindowsService;
  // ---- secret room and tax (ui-secret-tax, M7)
  /** The secret-room, tax and 1:1 line-up windows (WP-UI-56, WP-UI-57): the bid and its status, the line-up editor (702 and, for the 1:1 combat, 672), the board, the entrance question, the cancel confirmation, the member list of the war and the two tax windows; `openOffer` ... `openCheckTaxRate`, `closeAll`. */
  readonly secretRoom: SecretRoomWindowsService;
  // ---- rainbow race and quiz (ui-rainbow)
  /** The rainbow-race windows (WP-UI-60..62): the application, the info / rule / prize panels, the previous ranking, the race button, the progress list and the seven game windows with their end boxes; `openOffer` ... `openLadder`, `openEndBox`, `closeAll`. */
  readonly rainbow: RainbowWindowsService;
  // ---- mini-games (ui-minigames, M7)
  /** The NPC mini-game windows (WP-UI-63): Kawibawibo with its win window and confirm, the find-word, puzzle and dice games with the stake and result boxes; `openKawibawibo` ... `openDiceMessage`, `closeAll`. */
  readonly minigame: MiniGameWindowsService;
  /** The player-trade windows (WP-UI-26): the trade window, the request and the final confirmation; `setPorts`, `openRequest`, `openFinalConfirm`. */
  readonly trade: TradeWindowsService;
  /** The personal-shop windows (WP-UI-26, -37): the shop window, the price and purchase windows and the chat room; `setPorts`, `openSell`, `openBuy`, `toggleChat`. */
  readonly vendor: VendorWindowsService;
  /** The readable-item windows (WP-UI-37): the book, the scroll and the letter; `open( kind, request )`. */
  readonly readable: ReadableWindowsService;
  /** The navigator, the TELE16 teleporter, the inspection window, the collecting panel and the world-map overlay (WP-UI-36). */
  readonly nav: NavWindowsService;
  /** The character core windows (WP-UI-21): inventory, status, character, motion, the stat confirm; `setPorts`, `statConfirm`. */
  readonly char: CharService;
  /** The NPC dialog and shop windows (WP-UI-24): dialog, shop, buy / sell confirmations, quick sell, the mover menu popup. */
  readonly npc: NpcWindowsService;
  /** The typed view of 45's options (WP-UI-09): `uiScale`, `windowAlpha`, `gamma`, `bright`, `contrast`, ... */
  readonly options: UiOptionsStore;
  /** UI scale and virtual viewport (§16): `observeBrowser()` follows the browser window; `UiRoot scaler={...}`. */
  readonly scaler: UiScaler;
  /** The screen ramp (§7.8): `UiRoot ramp={...}` puts the filter on the root; cursors are ramped through it. */
  readonly ramp: UiRamp;
  /** Window-rect persistence (§17): installed as `wm.rectStore`; `saveAll()` when the world is left. */
  readonly regInfo: RegInfoStore;
  /** The `__flyffDebug.ui` section (§21.2): `ctx.probe.extend('ui', ui.probe)`. */
  readonly probe: UiProbe;
  /**
   * Loads the chunks of the dialog windows (`ui/wnd/lazy.ts`); resolves with the names of the classes that failed to load. The game starts it right
   * after the UI exists (the title screens do not wait for it).
   */
  preloadWindows(): Promise<string[]>;
  /** `true` once the dialog windows are loaded (or the preload has finished): the loading gate of the world waits for it. */
  windowsReady(): boolean;
  /** Detaches the window-manager listeners of the services (tests, HMR). */
  dispose(): void;
}

export interface CreateUiOptions {
  /** Virtual viewport (default 1280 x 720); `taskbarHeight` is subtracted from the layout rect. */
  viewport?: { width: number; height: number; taskbarHeight?: number };
  /** Ramp options: `false` = `ramp=off` (no filter, unramped cursors), e.g. for deterministic screenshots. */
  ramp?: boolean;
  /** How cursor images are decoded and re-encoded for the ramp (default: the browser canvas; `null`: cursors stay unramped). */
  cursorImageIo?: CursorImageIo | null;
  /** Extra options of the debug probe (the fixtures' opener, the mock-port action log, the Esc step log). */
  probe?: UiProbeOptions;
}

export function createUi(host: UiHost, options: CreateUiOptions = {}): UiApi {
  const log = host.log ?? createLogger({ console: false });
  // the bundles are read lazily: `UiHost.bundles.*` are getters that return `undefined` until the bundle is loaded
  const registry = LayoutRegistry.fromSupplier(() => host.bundles.layouts);
  const images = new UiImageCache({
    urls: host.assets,
    decode: host.decode ?? createBrowserDecoder(),
    warn: (m) => {
      log.warn('ui', m);
    },
  });
  const baker = new SkinBaker(images);
  const atlases = new UiAtlases(() => host.bundles.atlases, images);
  const wm = new WindowManager({
    registry,
    images,
    baker,
    host,
    log,
    ...(options.viewport ? { viewport: options.viewport } : {}),
  });
  const tooltip = new TooltipService({ wm, host });
  const cursor = new CursorManager(host.assets);
  const msgbox = new MessageBoxService(wm);
  const uiOptions = new UiOptionsStore(host.options);
  const scaler = new UiScaler({ wm, options: uiOptions });
  const ramp = new UiRamp({
    options: uiOptions,
    enabled: options.ramp ?? true,
    ...(options.cursorImageIo === undefined ? {} : { io: options.cursorImageIo }),
  });
  const regInfo = new RegInfoStore({
    idPlayer: () => host.playerId?.() ?? null,
    layout: () => wm.layoutRect,
    viewport: () => wm.viewport,
    frames: () => wm.order(),
    log,
  });
  wm.rectStore = regInfo;
  const taskbar = new Taskbar({
    wm,
    applets: wm.applets,
    images,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
    features: () => host.features,
    isGm: () => host.isGm?.() ?? false,
    setInset: (h) => {
      scaler.setTaskbarHeight(h);
    },
  });
  wm.appletHooks = {
    ...wm.appletHooks,
    createTaskbar: () => {
      taskbar.create();
    },
  };
  const session = new SessionService({
    wm,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const chat = new ChatService({
    wm,
    options: uiOptions,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const char = new CharService({
    wm,
    tooltip,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const dialogs = new ItemDialogsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const duel = new DuelWindowsService({ wm });
  const itemUse = new ItemUseWindowsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const appearance = new AppearanceWindowsService({ wm, tooltip });
  const bank = new BankWindowsService({
    wm,
    tooltip,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const pockets = new PocketWindowsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const mail = new MailWindowsService({
    wm,
    tooltip,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const ultimate = new UltimateWindowsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const exchange = new ExchangeWindowsService({ wm });
  const upgrade = new UpgradeWindowsService({ wm, tooltip });
  const skills = new SkillWindowsService({ wm, tooltip, atlases });
  const jobs = new JobWindowsService({ wm });
  const npc = new NpcWindowsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const nav = new NavWindowsService({
    wm,
    tooltip,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const pets = new PetWindowsService({ wm, tooltip });
  const messenger = new MessengerWindowsService({
    wm,
    tooltip,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const guild = new GuildWindowsService({
    wm,
    tooltip,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const siege = new SiegeWindowsService({
    wm,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const quest = new QuestWindowsService({
    wm,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  // ---- lord (ui-lord, M7)
  const lord = new LordWindowsService({
    wm,
    tooltip,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const gc1to1 = new Gc1to1WindowsService({
    wm,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  // ---- housing / guild house (ui-house-quiz)
  const housing = new HousingWindowsService({
    wm,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const quiz = new QuizWindowsService({
    wm,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  // ---- secret room and tax (ui-secret-tax, M7)
  const secretRoom = new SecretRoomWindowsService({
    wm,
    msgbox,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  // ---- rainbow race and quiz (ui-rainbow)
  const rainbow = new RainbowWindowsService({
    wm,
    msgbox,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  // ---- mini-games (ui-minigames, M7)
  const minigame = new MiniGameWindowsService({
    wm,
    msgbox,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
    commas: (n) => groupThousands(String(Math.trunc(n))),
  });
  const couple = new CoupleWindowsService({
    wm,
    tooltip,
    atlases,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const party = new PartyWindowsService({
    wm,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const trade = new TradeWindowsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const vendor = new VendorWindowsService({
    wm,
    tooltip,
    getText: (tid) => host.bundles.texts?.text(tid) ?? '',
  });
  const readable = new ReadableWindowsService({ wm });
  const offs = [
    tooltip.attach(),
    cursor.attach(wm),
    ramp.bindCursor(cursor),
    () => {
      scaler.dispose();
      ramp.dispose();
      if (wm.rectStore === regInfo) wm.rectStore = null;
    },
  ];
  const catalog = WINDOW_CATALOG;
  return {
    host,
    wm,
    registry,
    images,
    baker,
    atlases,
    catalog,
    tooltip,
    msgbox,
    cursor,
    applets: wm.applets,
    taskbar,
    chat,
    session,
    char,
    dialogs,
    duel,
    itemUse,
    appearance,
    ultimate,
    upgrade,
    exchange,
    bank,
    pockets,
    mail,
    skills,
    jobs,
    npc,
    party,
    messenger,
    guild,
    siege,
    couple,
    pets,
    quest,
    // ---- lord (ui-lord, M7)
    lord,
    gc1to1,
    // ---- housing / guild house (ui-house-quiz)
    housing,
    quiz,
    // ---- secret room and tax (ui-secret-tax, M7)
    secretRoom,
    // ---- rainbow race and quiz (ui-rainbow)
    rainbow,
    // ---- mini-games (ui-minigames, M7)
    minigame,
    nav,
    trade,
    vendor,
    readable,
    options: uiOptions,
    scaler,
    ramp,
    regInfo,
    probe: createUiProbe({ wm, registry, tooltip, catalog }, options.probe),
    preloadWindows: preloadLazyWindows,
    windowsReady: lazyWindowsReady,
    dispose: () => {
      for (const off of offs.splice(0)) off();
    },
    /** `UiApi.process(dtMs)` of the game loop: the UI tick. */
    process: () => {
      wm.tick(host.now());
    },
  };
}
