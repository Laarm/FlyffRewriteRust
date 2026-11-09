// The ports of the housing and guild-house windows (Plan/42 §19.4 N, 45 §28.10, §28.11; WP-UI-58, WP-UI-59). Every window takes ONE port or request object that carries what it shows and the callbacks of its buttons; what the
// callbacks send is the feature's (`features/events/{housing,guildHouse}`). The send table of CA3 (App. B names; the `// legacy-send:` markers live in the senders of the features):
//
//   WndHousing          BUTTON1 install   papering -> `SendHousingReqSetupFurniture`; furniture -> `CDeployManager::LoadToDeploy` (the click in the world sends `SendHousingReqSetupFurniture`)
//                       BUTTON2 remove    `SendHousingReqSetupFurniture{ setup: false }`
//   WndRoomList         opened            `SendHousingReqVisitableList`; BUTTON1 / a double click `SendHousingVisitRoom{ owner }`; BUTTON2 `SendHousingReqVisitableList`; BUTTON3 closes
//   WndQuitRoom         BUTTON1           `SendHousingGoOut`; BUTTON2 closes
//   WndGHMainMenu       BTN_SETUPPOWER the guild window on tab 4; BTN_FURNITURE / BTN_TELEPORTER the storage sections; BTN_STORAGE `SendOpenGuildBankWnd` / `SendCloseGuildBankWnd`; BTN_UPKEEP the upkeep window
//   WndGuildHousing     a row button      `CGuildHouseBase::SendClientToWorld( SETUP | REMOVE, ... )` (no `SendXxx` row of the csv), or the guild deploy
//   WndGHUpkeep         OK                `SendClientToWorld( UPKEEP, { item_id: days } )`
//   WndGHTeleporter     OK / FIRE         `SendClientToWorld( SETUP | LISTDROP )` (never instantiated by V15)

/** The nine texts and numbers of the housing list come from the feature (props resolved there); the window sorts and draws. */
import type { GhRowView, GhSection, HousingItemView } from './rows';

/** `APP_HOUSING`: the furniture list of the own mini room (the applet; the port is installed with `ui.housing.setHousingPort`). */
export interface HousingPort {
  /** The list in the order it was received. */
  items(): readonly HousingItemView[];
  /** Unix seconds now (the remaining time column). */
  nowSec(): number;
  /** `pWorld->GetID() == WI_WORLD_MINIROOM && IsMyRoom()`, read when a row is selected. */
  canEdit(): boolean;
  /** `CDeployManager::IsReady()`: a furniture is being placed. */
  deploying(): boolean;
  /** `EndDeploy()`: the placement ends, nothing is sent. */
  endDeploy(): void;
  /** `SendHousingReqSetupFurniture{ itemId, setup: true, pos 0, angle 0 }` of a wallpaper or a carpet. */
  installPaper(itemId: number): void;
  /** `LoadToDeploy( dwLinkKind, ... )` and the world takes the focus (a furniture). */
  deploy(itemId: number): void;
  /** `SendHousingReqSetupFurniture{ itemId, setup: false }`. */
  remove(itemId: number): void;
}

/** `APP_MINIROOM_LIST`: the rooms the player may visit. */
export interface RoomListRequest {
  /** The owners (character ids) in the order received. */
  owners(): readonly number[];
  /** The name of a character, `null` while the player-data cache does not know it. */
  nameOf(id: number): string | null;
  /** `OnInitialUpdate`: `m_vecVisitable.clear()` and `SendHousingReqVisitableList()`. */
  opened(): void;
  /** BUTTON2: `SendHousingReqVisitableList()`. */
  refresh(): void;
  /** BUTTON1 or a double click: `SendHousingVisitRoom( owner )`. */
  visit(owner: number): void;
}

/** `APP_QUIT_ROOM`. */
export interface QuitRoomRequest {
  /** BUTTON1: `SendHousingGoOut()`. */
  goOut(): void;
}

/** `APP_GH_MAIN`: the guild-house menu (opened with the world 212-214). */
export interface GhMainRequest {
  /** Unix seconds until the upkeep is paid, 0 = expired. */
  upkeepAt(): number;
  nowSec(): number;
  /** BTN_SETUPPOWER: toggles the guild window on its authority tab. */
  toggleSetupPower(): void;
  /** BTN_FURNITURE / BTN_TELEPORTER: opens, switches or closes the storage window in that section. */
  toggleStorage(section: GhSection): void;
  /** BTN_STORAGE: the guild bank window toggle. */
  toggleBank(): void;
  /** BTN_UPKEEP: the power check, then the upkeep window toggle. */
  toggleUpkeep(): void;
  /** The window went (the house is left): the storage, the guild window, the guild bank and a guild deploy go with it. */
  destroyed(): void;
}

/** `APP_GH_FURNITURE_STORAGE`: the furniture and teleporter storage (the applet; the port is installed with `ui.housing.setStoragePort`). */
export interface StoragePort {
  /** Every furniture of the house in list order (the window filters, sorts and draws). */
  rows(): readonly GhRowView[];
  nowSec(): number;
  /** Unix seconds until the upkeep is paid, 0 = expired. */
  upkeepAt(): number;
  /** `IsSetFurnitureChannel()`: the server's `bank_enabled` (event flag 2). */
  channelOn(): boolean;
  /** `hasPower( PF_GUILDHOUSE_FURNITURE )` inside a guild-house world: the row buttons are enabled. */
  canManage(): boolean;
  /** `GuildHouse->m_dwSelectedObjID` (`null` = `NULL_ID`). */
  selectedObjId(): number | null;
  /** A click on a row (or a re-selection after a refresh): the object id of the row, `null` for an uninstalled row. */
  select(objId: number | null): void;
  /** A row button: the feature decides (the channel text, the immediate request or the deploy). */
  rowButton(row: GhRowView): void;
  /** `GuildHouse->m_iMode == SETUP`: the row being placed (the storage window hides and the SHOWONE window shows it), `null` for none. */
  deployingRow(): GhRowView | null;
  /** The window was destroyed. */
  closed(): void;
}

/** `APP_CONFIRM_BUYEX` as the upkeep window. */
export interface UpkeepRequest {
  /** OK: `GuildHousePacketReq{ op 6, index 0, info{ item_id: days } }` (1..34). */
  pay(days: number): void;
}

/** `APP_GH_TELEPORTER` (a shell that V15 never instantiates): five slots, the installed one first. */
export interface TeleporterRequest {
  /** The five slots in order; `null` = empty. */
  slots(): readonly (TeleporterSlot | null)[];
  /** Unix seconds now. */
  nowSec(): number;
  /** OK: `SETUP`. */
  setup(slot: number): void;
  /** FIRE or a double click: `LISTDROP`. */
  fire(slot: number): void;
  /** An inventory teleporter was dropped on the window: `DoUseItem`. */
  dropped(objId: number): void;
}

export interface TeleporterSlot {
  readonly itemId: number;
  readonly icon: string;
  readonly name: string;
  readonly installed: boolean;
  /** Unix seconds when it expires, 0 = not started. */
  readonly expireAt: number;
  /** `dwAbilityMin`. */
  readonly abilityMin: number;
}

/** Texts of the windows. */
export interface HousingHost {
  readonly text: (tid: number) => string;
  /** A standard message box. */
  readonly box: (text: string) => void;
}
